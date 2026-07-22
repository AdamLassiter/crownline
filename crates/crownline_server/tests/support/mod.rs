use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use crownline_protocol::{
    ClientMessage, MatchSnapshot, MutationContext, PROTOCOL_VERSION, ReconnectToken, ServerMessage,
};
use crownline_server::{app_with_database, database::Durability, limits::ServerLimits};
use futures_util::{SinkExt as _, StreamExt as _};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use uuid::Uuid;

pub type TestSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub struct TestServer {
    pub address: SocketAddr,
    task: JoinHandle<()>,
}

impl TestServer {
    pub async fn start(database: &PathBuf) -> Self {
        let app = app_with_database(ServerLimits::default(), database, Durability::Normal)
            .expect("soak database must open");
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

    pub async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }

    pub fn http_url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    fn websocket_url(&self) -> String {
        format!("ws://{}/ws", self.address)
    }
}

pub fn database_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("crownline-{label}-{}.sqlite3", Uuid::new_v4()))
}

pub fn context(match_id: Uuid, revision: u64) -> MutationContext {
    MutationContext {
        match_id,
        expected_revision: revision,
        idempotency_key: Uuid::new_v4(),
    }
}

pub async fn send(socket: &mut TestSocket, message: &ClientMessage) {
    socket
        .send(Message::Text(
            serde_json::to_string(message).unwrap().into(),
        ))
        .await
        .unwrap();
}

pub async fn authenticate(
    server: &TestServer,
    room_code: &str,
    token: &ReconnectToken,
) -> TestSocket {
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

pub async fn next_server_message(socket: &mut TestSocket) -> ServerMessage {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("soak server response timed out")
            .expect("soak server closed unexpectedly")
            .expect("soak websocket response failed");
        let bytes = match message {
            Message::Text(text) => text.as_bytes().to_vec(),
            Message::Binary(bytes) => bytes.to_vec(),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
            Message::Close(frame) => panic!("soak server closed before response: {frame:?}"),
        };
        return serde_json::from_slice(&bytes).unwrap();
    }
}

pub async fn next_snapshot(socket: &mut TestSocket) -> MatchSnapshot {
    loop {
        if let ServerMessage::Snapshot { snapshot, .. } = next_server_message(socket).await {
            return *snapshot;
        }
    }
}

pub async fn next_result(socket: &mut TestSocket) -> ServerMessage {
    loop {
        let message = next_server_message(socket).await;
        if matches!(
            message,
            ServerMessage::Acknowledgement { .. } | ServerMessage::Error { .. }
        ) {
            return message;
        }
    }
}

pub fn acknowledged(message: ServerMessage) -> MatchSnapshot {
    match message {
        ServerMessage::Acknowledgement { result, .. } => result.snapshot,
        other => panic!("soak mutation must be acknowledged, received {other:?}"),
    }
}

pub fn remove_database_files(path: &Path) {
    for target in [
        path.to_path_buf(),
        path.with_extension("sqlite3-shm"),
        path.with_extension("sqlite3-wal"),
    ] {
        let _ = std::fs::remove_file(target);
    }
}
