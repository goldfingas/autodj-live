mod common;

use autodj_live_supervisor::{LiveSupervisor, Protection, RuntimeRecord, SupervisorError};
use common::*;
use std::time::Duration;
use tokio::time::{sleep, timeout};

#[tokio::test]
async fn live_owner_blocks_second_launch_then_drop_reattaches_same_engine_at_generation_two() {
    let config = config();
    let mut first = LiveSupervisor::launch(config.clone()).await.unwrap();
    let identity = first.identity();
    assert_eq!(
        LiveSupervisor::launch(config.clone()).await.err().unwrap(),
        SupervisorError::AlreadyRunning
    );
    assert_eq!(first.health().await.unwrap().supervisor_generation, 1);
    drop(first);
    exited(identity.sidecar_pid).await;
    assert!(running(identity.pid));
    let mut second = timeout(Duration::from_secs(5), async {
        loop {
            match LiveSupervisor::launch(config.clone()).await {
                Err(SupervisorError::AlreadyRunning) => sleep(Duration::from_millis(10)).await,
                result => break result.unwrap(),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(second.identity().pid, identity.pid);
    assert_eq!(second.identity().engine_id, identity.engine_id);
    assert_eq!(second.health().await.unwrap().supervisor_generation, 2);
    let (port, hello) = wire_credentials(&config.runtime.record_path());
    use autodj_live_protocol::{EngineCommand, EngineEvent, FrameCodec, ServerHello};
    let mut stale = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    FrameCodec::write(&mut stale, &hello).await.unwrap();
    let _: ServerHello = FrameCodec::read_typed(&mut stale).await.unwrap();
    FrameCodec::write(
        &mut stale,
        &EngineCommand::Attach {
            supervisor_generation: 1,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        FrameCodec::read_typed::<_, EngineEvent>(&mut stale)
            .await
            .unwrap(),
        EngineEvent::Error { .. }
    ));
    assert_eq!(second.health().await.unwrap().supervisor_generation, 2);
    second.shutdown().await.unwrap();
}

#[tokio::test]
async fn corrupt_record_or_authentication_failure_never_deletes_or_replaces_a_live_record() {
    let config = config();
    let session = LiveSupervisor::launch(config.clone()).await.unwrap();
    let identity = session.identity();
    let path = config.runtime.record_path();
    let original = std::fs::read(&path).unwrap();
    let mut wrong: serde_json::Value = serde_json::from_slice(&original).unwrap();
    wrong["engine_id"] = serde_json::json!(uuid::Uuid::new_v4());
    let wrong = serde_json::to_vec(&wrong).unwrap();
    std::fs::write(&path, &wrong).unwrap();
    assert_eq!(
        LiveSupervisor::launch(config.clone()).await.err(),
        Some(SupervisorError::Authentication)
    );
    assert_eq!(std::fs::read(&path).unwrap(), wrong);
    assert!(running(identity.pid));
    std::fs::write(&path, b"invalid record").unwrap();
    assert_eq!(
        LiveSupervisor::launch(config).await.err(),
        Some(SupervisorError::Discovery)
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid record");
    std::fs::write(&path, original).unwrap();
    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn concurrent_launches_yield_one_owner_and_each_new_launch_gets_new_credentials() {
    let config = config();
    let (first, second) = tokio::join!(
        LiveSupervisor::launch(config.clone()),
        LiveSupervisor::launch(config.clone())
    );
    let session = match (first, second) {
        (Ok(session), Err(SupervisorError::Busy | SupervisorError::AlreadyRunning))
        | (Err(SupervisorError::Busy | SupervisorError::AlreadyRunning), Ok(session)) => session,
        _ => panic!("exactly one supervisor must acquire ownership"),
    };
    let (_, first_credentials) = wire_credentials(&config.runtime.record_path());
    session.shutdown().await.unwrap();
    let next = LiveSupervisor::launch(config.clone()).await.unwrap();
    let (_, second_credentials) = wire_credentials(&config.runtime.record_path());
    assert!(first_credentials.token != second_credentials.token);
    assert!(first_credentials.session_id != second_credentials.session_id);
    next.shutdown().await.unwrap();
}

#[tokio::test]
async fn detach_keeps_record_until_lease_expiry_then_launch_replaces_stale_record() {
    let config = config();
    let path = config.runtime.record_path();
    let first = LiveSupervisor::launch(config.clone()).await.unwrap();
    let old = first.identity();
    first.detach().await.unwrap();
    assert!(path.exists());
    assert!(running(old.pid));
    // Real default lease: wait asynchronously, preserving the engine's binding contract.
    timeout(Duration::from_secs(34), async {
        while running(old.pid) {
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();
    assert!(path.exists());
    let second = LiveSupervisor::launch(config).await.unwrap();
    assert_ne!(second.identity().engine_id, old.engine_id);
    second.shutdown().await.unwrap();
}

#[tokio::test]
async fn portable_record_requires_owner_only_privacy_and_replacement_failure_preserves_record() {
    let mut config = config();
    config.runtime.protection = Protection::Portable;
    let session = LiveSupervisor::launch(config.clone()).await.unwrap();
    let path = config.runtime.record_path();
    let original = std::fs::read(&path).unwrap();
    let identity = session.identity();
    session.detach().await.unwrap();
    // Deny replacement without exposing or deleting the existing valid record.
    use std::os::windows::fs::OpenOptionsExt;
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert_eq!(
        LiveSupervisor::reattach(RuntimeRecord::load(config.clone()).unwrap())
            .await
            .err()
            .unwrap(),
        SupervisorError::Discovery
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(running(identity.pid));
    drop(locked);
    LiveSupervisor::reattach(RuntimeRecord::load(config).unwrap())
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
}

#[tokio::test]
async fn portable_discovery_refuses_a_record_with_a_public_acl() {
    let mut config = config();
    config.runtime.protection = Protection::Portable;
    let session = LiveSupervisor::launch(config.clone()).await.unwrap();
    let path = config.runtime.record_path();
    with_public_acl(&path, || {
        assert_eq!(
            RuntimeRecord::load(config.clone()).err(),
            Some(SupervisorError::Privacy)
        );
    });
    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn portable_discovery_refuses_a_public_runtime_directory() {
    let mut config = config();
    config.runtime.protection = Protection::Portable;
    let session = LiveSupervisor::launch(config.clone()).await.unwrap();
    let path = config.runtime.record_path();
    with_public_acl(path.parent().unwrap(), || {
        assert_eq!(
            RuntimeRecord::load(config.clone()).err(),
            Some(SupervisorError::Privacy)
        );
    });
    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn killing_supervisor_process_kills_sidecar_but_engine_reattaches() {
    use std::process::{Command, Stdio};
    let config = config();
    let root = config.runtime.record_path().parent().unwrap().to_owned();
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_supervisor-fixture"))
        .arg("supervisor")
        .arg(&root)
        .arg(&config.engine.executable)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let ready = root.with_extension("ready");
    timeout(Duration::from_secs(8), async {
        while !ready.exists() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let values = std::fs::read_to_string(ready).unwrap();
    let pids: Vec<u32> = values
        .split_whitespace()
        .map(|value| value.parse().unwrap())
        .collect();
    supervisor.kill().unwrap();
    supervisor.wait().unwrap();
    exited(pids[1]).await;
    assert!(running(pids[0]));
    let mut session = LiveSupervisor::reattach(RuntimeRecord::load(config).unwrap())
        .await
        .unwrap();
    assert_eq!(session.identity().pid, pids[0]);
    assert_eq!(session.health().await.unwrap().supervisor_generation, 2);
    session.shutdown().await.unwrap();
}
