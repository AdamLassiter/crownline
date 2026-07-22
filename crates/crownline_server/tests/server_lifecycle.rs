use std::{net::SocketAddr, path::PathBuf, time::Duration};

use crownline_core::{Action, ClockSettings, MatchState, start_clocks, state::OutcomeReason};
use crownline_protocol::{
    ActionRequest, ClientMessage, CreateRoomRequest, CreateRoomResponse, DrawCommand, ErrorCode,
    JoinRoomRequest, JoinRoomResponse, MatchSnapshot, MutationContext, PROTOCOL_VERSION,
    ReconnectToken, RematchCommand, ServerMessage,
};
use crownline_server::{app_with_database, database::Durability, limits::ServerLimits};
use futures_util::{SinkExt as _, StreamExt as _};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use uuid::Uuid;

type TestSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

struct TestServer {
    address: SocketAddr,
    task: JoinHandle<()>,
}

impl TestServer {
    async fn start(database: &PathBuf) -> Self {
        let app = app_with_database(ServerLimits::default(), database, Durability::Normal)
            .expect("ephemeral server database must open");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self { address, task }
    }

    async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }

    fn http_url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn websocket_url(&self) -> String {
        format!("ws://{}/ws", self.address)
    }
}

fn database_path() -> PathBuf {
    std::env::temp_dir().join(format!("crownline-server-test-{}.sqlite3", Uuid::new_v4()))
}

fn context(match_id: Uuid, revision: u64) -> MutationContext {
    MutationContext {
        match_id,
        expected_revision: revision,
        idempotency_key: Uuid::new_v4(),
    }
}

async fn send(socket: &mut TestSocket, message: &ClientMessage) {
    socket
        .send(Message::Text(
            serde_json::to_string(message).unwrap().into(),
        ))
        .await
        .unwrap();
}

async fn authenticate(server: &TestServer, room_code: &str, token: &ReconnectToken) -> TestSocket {
    let (mut socket, _) = connect_async(server.websocket_url()).await.unwrap();
    send(
        &mut socket,
        &ClientMessage::Authenticate {
            protocol_version: PROTOCOL_VERSION,
            room_code: room_code.to_owned(),
            reconnect_token: token.clone(),
        },
    )
    .await;
    socket
}

async fn next_server_message(socket: &mut TestSocket, forbidden_tokens: &[&str]) -> ServerMessage {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .expect("server response timed out")
            .expect("server closed unexpectedly")
            .expect("websocket response failed");
        let bytes = match message {
            Message::Text(text) => text.as_bytes().to_vec(),
            Message::Binary(bytes) => bytes.to_vec(),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
            Message::Close(frame) => panic!("server closed before response: {frame:?}"),
        };
        let body = String::from_utf8(bytes).unwrap();
        for token in forbidden_tokens {
            assert!(
                !body.contains(token),
                "authenticated server response leaked a reconnect token"
            );
        }
        return serde_json::from_str(&body).unwrap();
    }
}

async fn next_snapshot(socket: &mut TestSocket, forbidden_tokens: &[&str]) -> MatchSnapshot {
    loop {
        if let ServerMessage::Snapshot { snapshot, .. } =
            next_server_message(socket, forbidden_tokens).await
        {
            return *snapshot;
        }
    }
}

async fn next_mutation_result(socket: &mut TestSocket, forbidden_tokens: &[&str]) -> ServerMessage {
    loop {
        let message = next_server_message(socket, forbidden_tokens).await;
        if matches!(
            message,
            ServerMessage::Acknowledgement { .. } | ServerMessage::Error { .. }
        ) {
            return message;
        }
    }
}

fn acknowledgement(message: ServerMessage) -> MatchSnapshot {
    let ServerMessage::Acknowledgement { result, .. } = message else {
        panic!("mutation must be acknowledged");
    };
    result.snapshot
}

#[tokio::test]
async fn incompatible_protocol_is_actionable_before_a_seat_is_joined() {
    let database = database_path();
    let server = TestServer::start(&database).await;
    let client = reqwest::Client::new();
    let future_version = PROTOCOL_VERSION + 1;

    let response = client
        .post(server.http_url("/rooms"))
        .json(&CreateRoomRequest {
            protocol_version: future_version,
            player_name: "Future host".to_owned(),
            scenario_id: "introductory-crossing".to_owned(),
            clock: None,
        })
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UPGRADE_REQUIRED);
    assert!(matches!(
        response.json::<ServerMessage>().await.unwrap(),
        ServerMessage::Error {
            code: ErrorCode::IncompatibleProtocol,
            ..
        }
    ));

    let created: CreateRoomResponse = client
        .post(server.http_url("/rooms"))
        .json(&CreateRoomRequest {
            protocol_version: PROTOCOL_VERSION,
            player_name: "Current host".to_owned(),
            scenario_id: "introductory-crossing".to_owned(),
            clock: None,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let future_join = client
        .post(server.http_url("/rooms/join"))
        .json(&JoinRoomRequest {
            protocol_version: future_version,
            player_name: "Future guest".to_owned(),
            room_code: created.room_code.clone(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(future_join.status(), reqwest::StatusCode::UPGRADE_REQUIRED);
    assert!(matches!(
        future_join.json::<ServerMessage>().await.unwrap(),
        ServerMessage::Error {
            code: ErrorCode::IncompatibleProtocol,
            ..
        }
    ));
    client
        .post(server.http_url("/rooms/join"))
        .json(&JoinRoomRequest {
            protocol_version: PROTOCOL_VERSION,
            player_name: "Current guest".to_owned(),
            room_code: created.room_code.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let (mut socket, _) = connect_async(server.websocket_url()).await.unwrap();
    socket
        .send(Message::Text(
            serde_json::json!({
                "type": "authenticate",
                "protocol_version": future_version,
                "room_code": created.room_code,
                "reconnect_token": created.reconnect_token,
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    assert!(matches!(
        next_server_message(&mut socket, &[]).await,
        ServerMessage::Error {
            code: ErrorCode::IncompatibleProtocol,
            message,
            ..
        } if message.contains("Client protocol") && message.contains("server protocol")
    ));

    server.stop().await;
    for path in [
        database.clone(),
        database.with_extension("sqlite3-shm"),
        database.with_extension("sqlite3-wal"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "the test intentionally keeps one two-client lifecycle and its identities linear"
)]
async fn two_clients_cover_lifecycle_concurrency_reconnect_and_restart() {
    let database = database_path();
    let mut server = TestServer::start(&database).await;
    let client = reqwest::Client::new();
    let created: CreateRoomResponse = client
        .post(server.http_url("/rooms"))
        .json(&CreateRoomRequest {
            protocol_version: PROTOCOL_VERSION,
            player_name: "North tester".to_owned(),
            scenario_id: "introductory-crossing".to_owned(),
            clock: Some(ClockSettings {
                base_minutes: 1,
                increment_seconds: 0,
            }),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let joined: JoinRoomResponse = client
        .post(server.http_url("/rooms/join"))
        .json(&JoinRoomRequest {
            protocol_version: PROTOCOL_VERSION,
            player_name: "South tester".to_owned(),
            room_code: created.room_code.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created.match_id, joined.match_id);
    assert!(!format!("{created:?}{joined:?}").contains(created.reconnect_token.expose()));
    assert!(!format!("{created:?}{joined:?}").contains(joined.reconnect_token.expose()));
    let secrets = [
        created.reconnect_token.expose(),
        joined.reconnect_token.expose(),
    ];

    let invalid_token = ReconnectToken::issued("invalid-test-credential".to_owned());
    let mut unauthorized = authenticate(&server, &created.room_code, &invalid_token).await;
    assert!(matches!(
        next_server_message(&mut unauthorized, &secrets).await,
        ServerMessage::Error { .. }
    ));

    let mut north = authenticate(&server, &created.room_code, &created.reconnect_token).await;
    let mut south = authenticate(&server, &created.room_code, &joined.reconnect_token).await;
    south
        .send(Message::Text("{malformed".into()))
        .await
        .unwrap();
    assert!(matches!(
        next_mutation_result(&mut south, &secrets).await,
        ServerMessage::Error { .. }
    ));
    send(
        &mut north,
        &ClientMessage::Ready {
            protocol_version: PROTOCOL_VERSION,
            context: context(created.match_id, 0),
        },
    )
    .await;
    send(
        &mut south,
        &ClientMessage::Ready {
            protocol_version: PROTOCOL_VERSION,
            context: context(created.match_id, 0),
        },
    )
    .await;
    let initial = next_snapshot(&mut south, &secrets).await;
    assert_eq!(initial.revision, 0);

    let hold_key = Uuid::new_v4();
    let hold = ClientMessage::Action {
        protocol_version: PROTOCOL_VERSION,
        request: ActionRequest {
            context: MutationContext {
                match_id: created.match_id,
                expected_revision: 0,
                idempotency_key: hold_key,
            },
            action: Action::Hold {
                player: joined.seat,
            },
        },
    };
    send(&mut south, &hold).await;
    let first = acknowledgement(next_mutation_result(&mut south, &secrets).await);
    assert_eq!(first.revision, 1);
    send(&mut south, &hold).await;
    let duplicate = acknowledgement(next_mutation_result(&mut south, &secrets).await);
    assert_eq!(
        duplicate, first,
        "duplicate key must return original result"
    );

    let mut competing_north =
        authenticate(&server, &created.room_code, &created.reconnect_token).await;
    let first_competitor = ClientMessage::Action {
        protocol_version: PROTOCOL_VERSION,
        request: ActionRequest {
            context: context(created.match_id, 1),
            action: Action::Hold {
                player: created.seat,
            },
        },
    };
    let second_competitor = ClientMessage::Action {
        protocol_version: PROTOCOL_VERSION,
        request: ActionRequest {
            context: context(created.match_id, 1),
            action: Action::Hold {
                player: created.seat,
            },
        },
    };
    tokio::join!(
        send(&mut north, &first_competitor),
        send(&mut competing_north, &second_competitor)
    );
    let (left, right) = tokio::join!(
        next_mutation_result(&mut north, &secrets),
        next_mutation_result(&mut competing_north, &secrets)
    );
    let accepted = [&left, &right]
        .into_iter()
        .filter(|message| matches!(message, ServerMessage::Acknowledgement { .. }))
        .count();
    assert_eq!(accepted, 1, "same-revision commands commit exactly once");
    let revision_two = match (left, right) {
        (ServerMessage::Acknowledgement { result, .. }, _)
        | (_, ServerMessage::Acknowledgement { result, .. }) => result.snapshot,
        _ => panic!("one concurrent command must be accepted"),
    };
    assert_eq!(revision_two.revision, 2);
    let before_restart_clock = revision_two.state.clocks.unwrap().south_millis;

    drop(south);
    drop(north);
    drop(competing_north);
    server.stop().await;
    server = TestServer::start(&database).await;
    let mut south = authenticate(&server, &created.room_code, &joined.reconnect_token).await;
    let mut north = authenticate(&server, &created.room_code, &created.reconnect_token).await;
    let restored = next_snapshot(&mut south, &secrets).await;
    assert_eq!(restored.revision, revision_two.revision);
    assert_eq!(restored.state_hash, revision_two.state_hash);
    assert_eq!(
        restored.state.canonical_hash().unwrap(),
        restored.state_hash
    );

    send(
        &mut south,
        &ClientMessage::Draw {
            protocol_version: PROTOCOL_VERSION,
            context: context(created.match_id, 2),
            command: DrawCommand::Offer,
        },
    )
    .await;
    let offered = acknowledgement(next_mutation_result(&mut south, &secrets).await);
    assert!(
        offered.state.clocks.unwrap().south_millis < before_restart_clock,
        "the first post-restart mutation must charge persisted deadline downtime"
    );
    send(
        &mut north,
        &ClientMessage::Draw {
            protocol_version: PROTOCOL_VERSION,
            context: context(created.match_id, offered.revision),
            command: DrawCommand::Accept,
        },
    )
    .await;
    let terminal = acknowledgement(next_mutation_result(&mut north, &secrets).await);
    assert_eq!(
        terminal.state.outcome.unwrap().reason,
        OutcomeReason::AgreedDraw
    );

    send(
        &mut south,
        &ClientMessage::Rematch {
            protocol_version: PROTOCOL_VERSION,
            context: context(created.match_id, terminal.revision),
            command: RematchCommand::Request,
        },
    )
    .await;
    send(
        &mut north,
        &ClientMessage::Rematch {
            protocol_version: PROTOCOL_VERSION,
            context: context(created.match_id, terminal.revision),
            command: RematchCommand::Accept,
        },
    )
    .await;
    tokio::time::sleep(Duration::from_millis(25)).await;
    let mut rematch_south =
        authenticate(&server, &created.room_code, &joined.reconnect_token).await;
    let rematch = next_snapshot(&mut rematch_south, &secrets).await;
    assert_ne!(rematch.match_id, created.match_id);
    assert_eq!(rematch.revision, 0);
    let scenario =
        ron::from_str(include_str!("../../../assets/scenarios/introductory.ron")).unwrap();
    let expected_rematch = start_clocks(
        &MatchState::from_scenario(&scenario).unwrap(),
        ClockSettings {
            base_minutes: 1,
            increment_seconds: 0,
        },
    )
    .unwrap();
    assert_eq!(rematch.state, expected_rematch);

    send(
        &mut rematch_south,
        &ClientMessage::Action {
            protocol_version: PROTOCOL_VERSION,
            request: ActionRequest {
                context: context(rematch.match_id, 0),
                action: Action::Resign {
                    player: joined.seat,
                },
            },
        },
    )
    .await;
    let resigned = acknowledgement(next_mutation_result(&mut rematch_south, &secrets).await);
    assert_eq!(
        resigned.state.outcome.unwrap().reason,
        OutcomeReason::Resignation
    );

    server.stop().await;
    for path in [
        database.clone(),
        database.with_extension("sqlite3-shm"),
        database.with_extension("sqlite3-wal"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}
