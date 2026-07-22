mod support;

use std::{path::Path, time::Instant};

use crownline_core::{Action, ClockSettings, state::OutcomeReason};
use crownline_protocol::{
    ActionRequest, ClientMessage, CreateRoomRequest, CreateRoomResponse, DrawCommand,
    JoinRoomRequest, JoinRoomResponse, PROTOCOL_VERSION, ReconnectToken, ServerMessage,
};
use futures_util::SinkExt as _;
use rusqlite::{Connection, params};
use tokio_tungstenite::tungstenite::Message;

use support::{
    TestServer, acknowledged, authenticate, context, database_path, next_result, next_snapshot,
    remove_database_files, send,
};

const ROOM_COUNT: usize = 8;
const RSS_RETURN_BUDGET_KIB: u64 = 64 * 1024;

struct ReconnectFixture {
    room_code: String,
    token: ReconnectToken,
    expected_revision: u64,
    expected_hash: String,
}

#[cfg(target_os = "linux")]
fn resident_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

#[cfg(not(target_os = "linux"))]
const fn resident_kib() -> u64 {
    0
}

fn online_backup(source: &Path, target: &Path) -> (u128, u64, u64, u64) {
    let connection = Connection::open(source).unwrap();
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let started = Instant::now();
    connection
        .execute("VACUUM INTO ?1", params![target.to_string_lossy()])
        .unwrap();
    let elapsed = started.elapsed().as_millis();
    let backup = Connection::open(target).unwrap();
    let integrity: String = backup
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let actions = backup
        .query_row("SELECT COUNT(*) FROM actions", [], |row| row.get(0))
        .unwrap();
    let snapshots = backup
        .query_row("SELECT COUNT(*) FROM match_snapshots", [], |row| row.get(0))
        .unwrap();
    let matches = backup
        .query_row("SELECT COUNT(*) FROM matches", [], |row| row.get(0))
        .unwrap();
    (elapsed, actions, snapshots, matches)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "scheduled bounded multi-room soak"]
#[allow(
    clippy::too_many_lines,
    reason = "the soak keeps room churn, backup, restart, and cleanup in one auditable lifecycle"
)]
async fn scheduled_server_soak() {
    let baseline_rss = resident_kib();
    let database = database_path("soak");
    let backup = database_path("soak-backup");
    let server = TestServer::start(&database).await;
    let client = reqwest::Client::new();
    let mut active_fixture = None;
    let mut unread_subscriber = None;

    for room_index in 0..ROOM_COUNT {
        let created: CreateRoomResponse = client
            .post(server.http_url("/rooms"))
            .json(&CreateRoomRequest {
                protocol_version: PROTOCOL_VERSION,
                player_name: format!("North {room_index}"),
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
                player_name: format!("South {room_index}"),
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

        let mut north = authenticate(&server, &created.room_code, &created.reconnect_token).await;
        let mut south = authenticate(&server, &created.room_code, &joined.reconnect_token).await;
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
        let initial = next_snapshot(&mut south).await;

        if room_index == 0 {
            unread_subscriber = Some((
                authenticate(&server, &created.room_code, &created.reconnect_token).await,
                0,
            ));
        }
        if room_index == 1 {
            let invalid = ReconnectToken::issued("synthetic-invalid-soak-token".to_owned());
            let mut rejected = authenticate(&server, &created.room_code, &invalid).await;
            assert!(matches!(
                support::next_server_message(&mut rejected).await,
                ServerMessage::Error { .. }
            ));
        }
        if room_index == 2 {
            south.send(Message::Text("{invalid".into())).await.unwrap();
            assert!(matches!(
                next_result(&mut south).await,
                ServerMessage::Error { .. }
            ));
        }

        send(
            &mut south,
            &ClientMessage::Action {
                protocol_version: PROTOCOL_VERSION,
                request: ActionRequest {
                    context: context(created.match_id, initial.revision),
                    action: Action::Hold {
                        player: joined.seat,
                    },
                },
            },
        )
        .await;
        let first = acknowledged(next_result(&mut south).await);

        drop(south);
        let mut south = authenticate(&server, &created.room_code, &joined.reconnect_token).await;
        let reconnected = next_snapshot(&mut south).await;
        assert_eq!(reconnected.state_hash, first.state_hash);

        send(
            &mut north,
            &ClientMessage::Action {
                protocol_version: PROTOCOL_VERSION,
                request: ActionRequest {
                    context: context(created.match_id, reconnected.revision),
                    action: Action::Hold {
                        player: created.seat,
                    },
                },
            },
        )
        .await;
        let second = acknowledged(next_result(&mut north).await);

        let mut latest = second;
        if room_index == 0 {
            let offering_player = latest.state.active_player;
            for _ in 0..20 {
                let offering_socket = if offering_player == created.seat {
                    &mut north
                } else {
                    &mut south
                };
                send(
                    offering_socket,
                    &ClientMessage::Draw {
                        protocol_version: PROTOCOL_VERSION,
                        context: context(created.match_id, latest.revision),
                        command: DrawCommand::Offer,
                    },
                )
                .await;
                latest = acknowledged(next_result(offering_socket).await);

                let answering_player = offering_player.opponent();
                let answering_socket = if answering_player == created.seat {
                    &mut north
                } else {
                    &mut south
                };
                send(
                    answering_socket,
                    &ClientMessage::Draw {
                        protocol_version: PROTOCOL_VERSION,
                        context: context(created.match_id, latest.revision),
                        command: DrawCommand::Reject,
                    },
                )
                .await;
                latest = acknowledged(next_result(answering_socket).await);
            }
            assert!(latest.revision >= 40);
            unread_subscriber.as_mut().unwrap().1 = latest.revision;
        }

        if room_index % 2 == 0 && room_index != 0 {
            send(
                &mut south,
                &ClientMessage::Action {
                    protocol_version: PROTOCOL_VERSION,
                    request: ActionRequest {
                        context: context(created.match_id, latest.revision),
                        action: Action::Resign {
                            player: joined.seat,
                        },
                    },
                },
            )
            .await;
            let terminal = acknowledged(next_result(&mut south).await);
            assert_eq!(
                terminal.state.outcome.unwrap().reason,
                OutcomeReason::Resignation
            );
        } else if active_fixture.is_none() {
            active_fixture = Some(ReconnectFixture {
                room_code: created.room_code,
                token: joined.reconnect_token,
                expected_revision: latest.revision,
                expected_hash: latest.state_hash,
            });
        }
    }

    let (mut slow_subscriber, expected_revision) = unread_subscriber
        .take()
        .expect("slow subscriber was created");
    loop {
        let observed = next_snapshot(&mut slow_subscriber).await;
        if observed.revision == expected_revision {
            break;
        }
        assert!(observed.revision < expected_revision);
    }
    drop(slow_subscriber);

    let query_started = Instant::now();
    let live_reader = Connection::open(&database).unwrap();
    let live_actions: u64 = live_reader
        .query_row("SELECT COUNT(*) FROM actions", [], |row| row.get(0))
        .unwrap();
    let query_micros = query_started.elapsed().as_micros();
    drop(live_reader);
    let (backup_millis, backup_actions, snapshots, matches) = online_backup(&database, &backup);
    assert_eq!(backup_actions, live_actions);
    assert_eq!(matches, u64::try_from(ROOM_COUNT).unwrap());
    assert_eq!(snapshots, backup_actions + matches);
    let database_bytes = std::fs::metadata(&backup).unwrap().len();
    assert!(database_bytes < 256 * 1024 + backup_actions * 64 * 1024);

    server.stop().await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let after_stop_rss = resident_kib();
    if baseline_rss > 0 {
        assert!(
            after_stop_rss <= baseline_rss + RSS_RETURN_BUDGET_KIB,
            "RSS did not return near baseline: {baseline_rss} -> {after_stop_rss} KiB"
        );
    }

    let restored_server = TestServer::start(&backup).await;
    let fixture = active_fixture.expect("at least one active room must survive backup");
    let mut restored = authenticate(&restored_server, &fixture.room_code, &fixture.token).await;
    let snapshot = next_snapshot(&mut restored).await;
    assert_eq!(snapshot.revision, fixture.expected_revision);
    assert_eq!(snapshot.state_hash, fixture.expected_hash);
    assert_eq!(
        snapshot.state.canonical_hash().unwrap(),
        fixture.expected_hash
    );
    restored_server.stop().await;

    println!(
        "rooms={ROOM_COUNT} actions={backup_actions} snapshots={snapshots} db_bytes={database_bytes} query_us={query_micros} backup_ms={backup_millis} rss_before_kib={baseline_rss} rss_after_kib={after_stop_rss}"
    );
    remove_database_files(&database);
    remove_database_files(&backup);
}
