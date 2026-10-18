use std::{process::Stdio, time::Duration};

use autodj_live_protocol::{
    BearerToken, ClientHello, EngineCommand, EngineEvent, FrameCodec, HealthSnapshot,
    ProtocolVersion, ServerHello,
};
use serde::de::DeserializeOwned;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    net::TcpStream,
    process::{Child, ChildStdout, Command},
    time::timeout,
};
use uuid::Uuid;

pub const TOKEN: &str = "abababababababababababababababababababababababababababababababab";
pub const SESSION: &str = "11111111-2222-3333-4444-555555555555";
pub const LIMIT: Duration = Duration::from_secs(5);

pub struct Engine {
    pub child: Child,
    pub descriptor: serde_json::Value,
    stdout: BufReader<ChildStdout>,
    first_line: String,
}

pub fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_autodj-live-engine"));
    command
        .env("AUTODJ_LIVE_TOKEN", TOKEN)
        .env("AUTODJ_LIVE_SESSION_ID", SESSION)
        .env_remove("AUTODJ_LIVE_RECONNECT_MS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

impl Engine {
    pub async fn start(lease_ms: Option<u64>) -> Self {
        let mut command = command();
        if let Some(ms) = lease_ms {
            command.env("AUTODJ_LIVE_RECONNECT_MS", ms.to_string());
        }
        let mut child = command.spawn().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut first_line = String::new();
        assert!(
            timeout(LIMIT, stdout.read_line(&mut first_line))
                .await
                .unwrap()
                .unwrap()
                > 0
        );
        let descriptor: serde_json::Value = serde_json::from_str(&first_line).unwrap();
        assert_eq!(
            descriptor["protocol"],
            serde_json::json!({"major": 1, "minor": 0})
        );
        assert_eq!(
            descriptor["pid"].as_u64().unwrap(),
            u64::from(child.id().unwrap())
        );
        assert_eq!(descriptor.as_object().unwrap().len(), 4);
        assert!(descriptor["port"].as_u64().unwrap() > 0);
        Uuid::parse_str(descriptor["engine_id"].as_str().unwrap()).unwrap();
        Self {
            child,
            descriptor,
            stdout,
            first_line,
        }
    }

    pub fn port(&self) -> u16 {
        self.descriptor["port"]
            .as_u64()
            .unwrap()
            .try_into()
            .unwrap()
    }

    pub async fn socket(&self) -> TcpStream {
        timeout(LIMIT, TcpStream::connect(("127.0.0.1", self.port())))
            .await
            .unwrap()
            .unwrap()
    }

    pub async fn client(&self) -> TcpStream {
        let mut socket = self.socket().await;
        FrameCodec::write(&mut socket, &hello()).await.unwrap();
        let response: ServerHello = receive(&mut socket).await;
        assert_eq!(response.version, ProtocolVersion::CURRENT);
        assert_eq!(response.engine_id.to_string(), self.descriptor["engine_id"]);
        assert_eq!(
            u64::from(response.pid),
            self.descriptor["pid"].as_u64().unwrap()
        );
        socket
    }

    pub async fn finish(mut self) {
        assert!(
            timeout(LIMIT, self.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        let mut extra = String::new();
        self.stdout.read_to_string(&mut extra).await.unwrap();
        assert!(extra.is_empty(), "only the descriptor belongs on stdout");
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .await
            .unwrap();
        assert!(!self.first_line.contains(TOKEN));
        assert!(!stderr.contains(TOKEN));
        assert!(!stderr.contains(&format!("{:?}", [0xab_u8; 32])));
    }
}

pub fn hello() -> ClientHello {
    ClientHello {
        version: ProtocolVersion::CURRENT,
        session_id: Uuid::parse_str(SESSION).unwrap(),
        token: BearerToken::new([0xab; 32]),
    }
}

pub async fn receive<T: DeserializeOwned>(socket: &mut TcpStream) -> T {
    timeout(LIMIT, FrameCodec::read_typed(socket))
        .await
        .unwrap()
        .unwrap()
}

pub async fn request(socket: &mut TcpStream, command: EngineCommand) -> EngineEvent {
    FrameCodec::write(socket, &command).await.unwrap();
    receive(socket).await
}

pub async fn health(socket: &mut TcpStream) -> HealthSnapshot {
    match request(socket, EngineCommand::Health).await {
        EngineEvent::Health(health) => health,
        other => panic!("expected health, got {other:?}"),
    }
}
