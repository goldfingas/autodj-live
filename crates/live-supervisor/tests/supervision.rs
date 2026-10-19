mod common;

use std::time::{Duration, Instant};

use autodj_live_supervisor::{LiveSupervisor, SupervisorError};
use common::*;

#[tokio::test]
async fn one_secret_pair_authenticates_both_children_and_normal_shutdown_reaps_them() {
    let config = config();
    let path = config.runtime.record_path();
    let mut session = LiveSupervisor::launch(config).await.unwrap();
    let identity = session.identity();
    assert_eq!(session.health().await.unwrap().supervisor_generation, 1);
    assert!(session.sidecar_healthy().await.unwrap());
    let record = std::fs::read_to_string(&path).unwrap();
    assert!(record.contains("protected_token"));
    assert!(!record.contains("\"token\""));
    session.shutdown().await.unwrap();
    exited(identity.pid).await;
    exited(identity.sidecar_pid).await;
    assert!(!path.exists());
}

#[tokio::test]
async fn malformed_noisy_oversized_and_missing_descriptors_reap_child_with_fixed_errors() {
    for (mode, expected) in [
        ("malformed", SupervisorError::InvalidDescriptor),
        ("noisy", SupervisorError::InvalidDescriptor),
        ("no-descriptor", SupervisorError::InvalidDescriptor),
        ("oversized", SupervisorError::DescriptorTooLarge),
        ("timeout", SupervisorError::StartupTimeout),
    ] {
        let mut config = config();
        let marker = directory().with_extension("pid");
        config.engine = fixture(mode, &marker);
        let started = Instant::now();
        let error = LiveSupervisor::launch(config).await.err().unwrap();
        assert_eq!(error, expected);
        assert!(started.elapsed() < Duration::from_secs(8));
        if mode == "timeout" {
            assert!(started.elapsed() >= Duration::from_secs(5));
        }
        exited(marker_pid(&marker)).await;
        assert!(!format!("{error:?} {error}").contains("secret"));
    }
}

#[tokio::test]
async fn stderr_flood_does_not_block_startup_and_extra_stdout_is_rejected() {
    let mut config = config();
    config.sidecar = fixture("stderr-flood", &directory().with_extension("pid"));
    let session = LiveSupervisor::launch(config).await.unwrap();
    assert!(session.sidecar_healthy().await.unwrap());
    session.shutdown().await.unwrap();

    let mut config = common::config();
    let marker = directory().with_extension("pid");
    config.engine = fixture("extra-line", &marker);
    assert!(LiveSupervisor::launch(config).await.is_err());
    exited(marker_pid(&marker)).await;
}

#[tokio::test]
async fn failed_sidecar_startup_stops_the_engine_and_leaves_no_record() {
    let mut config = config();
    let marker = directory().with_extension("pid");
    config.sidecar = fixture("malformed", &marker);
    let record = config.runtime.record_path();
    assert_eq!(
        LiveSupervisor::launch(config).await.err().unwrap(),
        SupervisorError::InvalidDescriptor
    );
    exited(marker_pid(&marker)).await;
    assert!(!record.exists());
}

#[tokio::test]
async fn executable_must_be_absolute_and_shell_scripts_are_refused() {
    let mut config = config();
    config.engine.executable = "cmd.exe".into();
    assert_eq!(
        LiveSupervisor::launch(config).await.err().unwrap(),
        SupervisorError::InvalidExecutable
    );
}

#[tokio::test]
async fn descriptor_accepts_exactly_eight_kib_and_rejects_a_fifth_field() {
    let mut config = config();
    config.sidecar = fixture("exact-limit", &directory().with_extension("pid"));
    LiveSupervisor::launch(config)
        .await
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    let mut config = common::config();
    config.sidecar = fixture("extra-field", &directory().with_extension("pid"));
    let result = LiveSupervisor::launch(config).await;
    let error = match result {
        Ok(session) => {
            session.shutdown().await.unwrap();
            None
        }
        Err(error) => Some(error),
    };
    assert_eq!(error, Some(SupervisorError::InvalidDescriptor));
}

#[tokio::test]
async fn bundled_python_sidecar_authenticates_and_all_redirector_processes_exit() {
    use autodj_live_supervisor::ChildCommand;
    let mut config = config();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    config.sidecar = ChildCommand::bundled_python(root.join(".venv/Scripts/python.exe"));
    let session = LiveSupervisor::launch(config).await.unwrap();
    assert!(session.sidecar_healthy().await.unwrap());
    let identity = session.identity();
    session.shutdown().await.unwrap();
    exited(identity.sidecar_pid).await;
    exited(identity.pid).await;
}
