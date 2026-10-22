mod common;

use std::time::{Duration, Instant};

use autodj_live_supervisor::{LiveSupervisor, SupervisorError};
use common::*;

#[tokio::test]
async fn engine_retry_failure_keeps_sidecar_alive_and_can_recover() {
    let mut config = config();
    config.runtime.protection = autodj_live_supervisor::Protection::Portable;
    let source_engine = config.engine.executable.clone();
    let retry_engine = directory().with_extension("exe");
    std::fs::copy(&source_engine, &retry_engine).unwrap();
    config.engine.executable = retry_engine.clone();
    let record = config.runtime.record_path();
    let mut session = LiveSupervisor::launch(config).await.unwrap();
    let original = session.identity();
    // A replacement cannot publish discovery while this exact path is a directory.
    std::fs::remove_file(&record).unwrap();
    std::fs::create_dir(&record).unwrap();
    assert!(session.restart_engine().await.is_err());
    assert!(running(original.sidecar_pid));
    assert!(session.sidecar_healthy().await.unwrap());
    assert!(session.health().await.is_err());
    // Windows refuses deletion of a mapped executable: both engine attempts must be reaped.
    std::fs::remove_file(&retry_engine).unwrap();
    assert!(session.restart_engine().await.is_err());
    assert!(running(original.sidecar_pid));
    std::fs::copy(&source_engine, &retry_engine).unwrap();
    std::fs::remove_dir(&record).unwrap();
    session.restart_engine().await.unwrap();
    assert_ne!(session.identity().engine_id, original.engine_id);
    assert_eq!(session.identity().sidecar_pid, original.sidecar_pid);
    assert!(session.health().await.is_ok());
    assert!(session.sidecar_healthy().await.unwrap());
    let replacement = session.identity();
    session.shutdown().await.unwrap();
    exited(original.pid).await;
    exited(replacement.pid).await;
    exited(original.sidecar_pid).await;
    std::fs::remove_file(&retry_engine).unwrap();
}

#[tokio::test]
async fn sidecar_retry_keeps_engine_attached() {
    let mut config = config();
    config.runtime.protection = autodj_live_supervisor::Protection::Portable;
    let mut session = LiveSupervisor::launch(config).await.unwrap();
    let original = session.identity();
    session.restart_sidecar().await.unwrap();
    assert_eq!(session.identity().engine_id, original.engine_id);
    assert_eq!(session.identity().pid, original.pid);
    assert_ne!(session.identity().sidecar_pid, original.sidecar_pid);
    assert_eq!(session.health().await.unwrap().supervisor_generation, 1);
    assert!(session.sidecar_healthy().await.unwrap());
    exited(original.sidecar_pid).await;
    session.shutdown().await.unwrap();
}

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
async fn active_supervisor_receives_engine_owned_audio_inventory() {
    let config = config();
    let mut session = LiveSupervisor::launch(config).await.unwrap();

    let discovery = session.audio_discovery().await.unwrap();

    assert!(
        discovery
            .endpoints
            .iter()
            .all(|endpoint| !endpoint.id.is_empty())
    );
    assert!(
        discovery
            .asio_drivers
            .iter()
            .all(|driver| !driver.registration.is_empty() && !driver.clsid.is_empty())
    );
    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn active_supervisor_can_probe_an_active_wasapi_endpoint() {
    let config = config();
    let mut session = LiveSupervisor::launch(config).await.unwrap();
    let discovery = session.audio_discovery().await.unwrap();
    if let Some(endpoint) = discovery
        .endpoints
        .iter()
        .find(|endpoint| endpoint.state == autodj_live_protocol::AudioEndpointState::Active)
    {
        let capabilities = session.probe_wasapi(&endpoint.id).await.unwrap();
        assert_eq!(capabilities.endpoint_id, endpoint.id);
        assert!(capabilities.channels > 0);
        assert!(capabilities.sample_rate_hz > 0);
        assert!(capabilities.bits_per_sample > 0);
        assert!(capabilities.default_period_100ns >= capabilities.minimum_period_100ns);
    }
    assert_eq!(
        session.probe_wasapi("missing-endpoint").await,
        Err(SupervisorError::Rejected)
    );
    session.shutdown().await.unwrap();
}

#[tokio::test]
async fn active_supervisor_can_initialize_and_release_an_active_wasapi_shared_buffer() {
    let config = config();
    let mut session = LiveSupervisor::launch(config).await.unwrap();
    let discovery = session.audio_discovery().await.unwrap();
    if let Some(endpoint) = discovery
        .endpoints
        .iter()
        .find(|endpoint| endpoint.state == autodj_live_protocol::AudioEndpointState::Active)
    {
        let initialized = session
            .initialize_wasapi_shared(&endpoint.id)
            .await
            .unwrap();
        assert_eq!(initialized.endpoint_id, endpoint.id);
        assert!(initialized.requested_period_frames > 0);
        assert!(initialized.buffer_frames > 0);
    }
    assert_eq!(
        session.initialize_wasapi_shared("missing-endpoint").await,
        Err(SupervisorError::Rejected)
    );
    session.shutdown().await.unwrap();
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

#[test]
fn bundled_python_ignores_hostile_python_environment() {
    let hostile = directory();
    let package = hostile.join("autodj");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("__init__.py"),
        "raise RuntimeError('hostile import')",
    )
    .unwrap();
    for hostile_home in [false, true] {
        // A separate test process avoids changing this parallel runner's environment.
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "bundled_python_sidecar_authenticates_and_all_redirector_processes_exit",
                "--nocapture",
            ])
            .current_dir(&hostile)
            .env("PYTHONPATH", &hostile)
            .env_remove("PYTHONHOME");
        if hostile_home {
            command.env("PYTHONHOME", &hostile);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
            "bundled sidecar must ignore hostile Python environment (PYTHONHOME={hostile_home}): {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    std::fs::remove_dir_all(hostile).unwrap();
}
