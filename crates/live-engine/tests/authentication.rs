mod common;

use autodj_live_protocol::{BearerToken, EngineCommand, EngineEvent, FrameCodec};
use common::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Duration, timeout},
};
use uuid::Uuid;

#[tokio::test]
async fn descriptor_is_flushed_once_and_listener_is_ipv4_loopback_only() {
    let engine = Engine::start(None).await;
    assert!(TcpStream::connect(("::1", engine.port())).await.is_err());
    let route = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    if route.connect("192.0.2.1:9").is_ok() {
        let address = route.local_addr().unwrap().ip();
        if !address.is_loopback() && !address.is_unspecified() {
            assert!(!matches!(
                timeout(
                    Duration::from_millis(250),
                    TcpStream::connect((address, engine.port()))
                )
                .await,
                Ok(Ok(_))
            ));
        }
    }
    let mut client = engine.client().await;
    let snapshot = health(&mut client).await;
    assert!(snapshot.orphaned);
    // Production startup uses a 30-second lease in the Windows monotonic uptime domain.
    assert_eq!(
        snapshot.lease_deadline_ns - snapshot.started_ns,
        30_000_000_000
    );
    // SAFETY: GetTickCount64 has no caller preconditions.
    let uptime_ns =
        unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() } * 1_000_000;
    assert!(snapshot.started_ns <= uptime_ns);
    assert!(uptime_ns - snapshot.started_ns < 5_000_000_000);
    assert_eq!(
        request(&mut client, EngineCommand::Shutdown).await,
        EngineEvent::Stopping
    );
    engine.finish().await;
}

#[tokio::test]
async fn unauthenticated_or_incompatible_clients_receive_no_bytes() {
    let engine = Engine::start(None).await;
    for variant in 0..5 {
        let mut socket = engine.socket().await;
        if variant == 0 {
            FrameCodec::write(&mut socket, &EngineCommand::Health)
                .await
                .unwrap();
        } else {
            let mut credentials = hello();
            match variant {
                1 => credentials.token = BearerToken::new([0xac; 32]),
                2 => credentials.session_id = Uuid::nil(),
                3 => credentials.version.major = 2,
                _ => {
                    socket
                        .write_all(&1_048_577_u32.to_be_bytes())
                        .await
                        .unwrap();
                }
            }
            if variant != 4 {
                FrameCodec::write(&mut socket, &credentials).await.unwrap();
            }
        }
        let mut response = [0; 1];
        let read = timeout(LIMIT, socket.read(&mut response)).await.unwrap();
        assert!(matches!(read, Ok(0)) || read.is_err());
    }
    // No bytes are sent merely because a TCP connection was accepted.
    let mut silent = engine.socket().await;
    assert!(
        timeout(Duration::from_millis(100), silent.read_u8())
            .await
            .is_err()
    );
    let mut client = engine.client().await;
    assert_eq!(health(&mut client).await.supervisor_generation, 0);
    assert_eq!(
        request(&mut client, EngineCommand::Shutdown).await,
        EngineEvent::Stopping
    );
    engine.finish().await;
}

#[tokio::test]
async fn startup_rejects_invalid_environment_without_echoing_input() {
    for (name, value) in [
        ("AUTODJ_LIVE_TOKEN", None),
        ("AUTODJ_LIVE_TOKEN", Some("ABCDEF")),
        ("AUTODJ_LIVE_TOKEN", Some("abab")),
        ("AUTODJ_LIVE_SESSION_ID", Some(TOKEN)),
        ("AUTODJ_LIVE_RECONNECT_MS", Some("0")),
        ("AUTODJ_LIVE_RECONNECT_MS", Some(TOKEN)),
    ] {
        let mut command = command();
        match value {
            Some(value) => {
                command.env(name, value);
            }
            None => {
                command.env_remove(name);
            }
        }
        let output = timeout(LIMIT, command.output()).await.unwrap().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8(output.stderr).unwrap();
        assert!(diagnostic.contains(name));
        assert!(!diagnostic.contains(TOKEN));
        assert!(!diagnostic.contains("ABCDEF"));
    }
}
