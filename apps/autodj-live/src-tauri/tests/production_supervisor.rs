use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use autodj_live_app::{
    AudioDiscovery, Component, FoundationLifecycle, ProductionSupervisor, ShellLifecycle,
    SupervisorBackend, SupervisorRuntime, SupervisorSession, WasapiCapabilities,
    WasapiSharedInitialization, resolve_child_paths,
};
use autodj_live_supervisor::{
    ChildCommand, LaunchConfig, Protection, RuntimeStore, SupervisorError,
};
use uuid::Uuid;

#[derive(Default)]
struct State {
    engine_id: Option<Uuid>,
    engine_healthy: bool,
    sidecar_healthy: bool,
    launches: usize,
    reattachments: usize,
    sidecar_restarts: usize,
    engine_restarts: usize,
    shutdowns: usize,
    stale_reattach: bool,
    reject_reattach: bool,
    reject_launch: bool,
}

#[derive(Clone)]
struct Runtime {
    state: Arc<Mutex<State>>,
}

struct Session {
    engine_id: Uuid,
    state: Arc<Mutex<State>>,
}

impl SupervisorSession for Session {
    fn engine_id(&self) -> Uuid {
        self.engine_id
    }

    async fn engine_healthy(&mut self) -> Result<(), SupervisorError> {
        self.state
            .lock()
            .unwrap()
            .engine_healthy
            .then_some(())
            .ok_or(SupervisorError::Connection)
    }

    async fn sidecar_healthy(&self) -> Result<bool, SupervisorError> {
        Ok(self.state.lock().unwrap().sidecar_healthy)
    }

    async fn restart_sidecar(&mut self) -> Result<(), SupervisorError> {
        let mut state = self.state.lock().unwrap();
        state.sidecar_restarts += 1;
        state.sidecar_healthy = false;
        if state.reject_reattach {
            return Err(SupervisorError::Spawn);
        }
        state.sidecar_healthy = true;
        Ok(())
    }

    async fn restart_engine(&mut self) -> Result<(), SupervisorError> {
        let mut state = self.state.lock().unwrap();
        state.engine_restarts += 1;
        state.engine_healthy = false;
        if state.reject_launch {
            return Err(SupervisorError::Spawn);
        }
        self.engine_id = Uuid::new_v4();
        state.engine_id = Some(self.engine_id);
        state.engine_healthy = true;
        Ok(())
    }

    async fn shutdown(self) -> Result<(), SupervisorError> {
        let mut state = self.state.lock().unwrap();
        state.shutdowns += 1;
        state.engine_id = None;
        state.engine_healthy = false;
        state.sidecar_healthy = false;
        Ok(())
    }

    async fn audio_discovery(&mut self) -> Result<AudioDiscovery, SupervisorError> {
        Ok(AudioDiscovery {
            endpoints: vec![],
            asio_drivers: vec![],
            issues: vec![],
        })
    }

    async fn probe_wasapi(
        &mut self,
        endpoint_id: &str,
    ) -> Result<WasapiCapabilities, SupervisorError> {
        Ok(WasapiCapabilities {
            endpoint_id: endpoint_id.into(),
            channels: 2,
            sample_rate_hz: 48_000,
            bits_per_sample: 32,
            default_period_100ns: 100_000,
            minimum_period_100ns: 30_000,
            exclusive_shared_mix_supported: false,
            exclusive_shared_format_sample_rates_hz: Some(vec![]),
            shared_mode_engine_periods: Some(autodj_live_protocol::SharedModeEnginePeriods {
                default_frames: 480,
                fundamental_frames: 4,
                minimum_frames: 48,
                maximum_frames: 480,
            }),
        })
    }

    async fn initialize_wasapi_shared(
        &mut self,
        endpoint_id: &str,
    ) -> Result<WasapiSharedInitialization, SupervisorError> {
        Ok(WasapiSharedInitialization {
            endpoint_id: endpoint_id.into(),
            requested_period_frames: 128,
            buffer_frames: 480,
        })
    }
}

impl SupervisorRuntime for Runtime {
    type Record = Uuid;
    type Session = Session;

    fn load(&mut self, _config: LaunchConfig) -> Result<Self::Record, SupervisorError> {
        self.state
            .lock()
            .unwrap()
            .engine_id
            .ok_or(SupervisorError::StaleRecord)
    }

    async fn reattach(&mut self, record: Self::Record) -> Result<Self::Session, SupervisorError> {
        let mut state = self.state.lock().unwrap();
        state.reattachments += 1;
        if state.stale_reattach {
            state.stale_reattach = false;
            state.engine_id = None;
            return Err(SupervisorError::StaleRecord);
        }
        if state.reject_reattach {
            return Err(SupervisorError::Authentication);
        }
        if state.engine_id != Some(record) {
            return Err(SupervisorError::StaleRecord);
        }
        state.engine_healthy = true;
        state.sidecar_healthy = true;
        Ok(Session {
            engine_id: record,
            state: self.state.clone(),
        })
    }

    async fn launch(&mut self, _config: LaunchConfig) -> Result<Self::Session, SupervisorError> {
        let mut state = self.state.lock().unwrap();
        if state.reject_launch {
            return Err(SupervisorError::Spawn);
        }
        state.launches += 1;
        let engine_id = Uuid::from_u128(state.launches as u128 + 100);
        state.engine_id = Some(engine_id);
        state.engine_healthy = true;
        state.sidecar_healthy = true;
        Ok(Session {
            engine_id,
            state: self.state.clone(),
        })
    }
}

fn config() -> LaunchConfig {
    LaunchConfig {
        engine: ChildCommand::new(PathBuf::from("engine.exe")),
        sidecar: ChildCommand::new(PathBuf::from("python.exe")),
        runtime: RuntimeStore::new(
            std::env::temp_dir().join(format!("autodj-app-test-{}", Uuid::new_v4())),
            Protection::Portable,
        ),
    }
}

fn backend(state: Arc<Mutex<State>>) -> ProductionSupervisor<Runtime> {
    ProductionSupervisor::with_runtime(config(), Runtime { state })
}

#[test]
fn release_child_paths_ignore_development_environment_overrides() {
    let resources = PathBuf::from(r"C:\Program Files\AutoDJ Live\resources");
    let (engine, python) = resolve_child_paths(
        resources.clone(),
        true,
        Some(PathBuf::from(r"C:\dev\engine.exe")),
        Some(PathBuf::from(r"C:\dev\python.exe")),
    );

    assert_eq!(engine, resources.join("autodj-live-engine.exe"));
    assert_eq!(python, resources.join("python").join("python.exe"));
}

#[tokio::test]
async fn reconnect_launches_after_task6_stale_but_not_other_reattach_errors() {
    let stale = Arc::new(Mutex::new(State {
        engine_id: Some(Uuid::from_u128(1)),
        engine_healthy: true,
        sidecar_healthy: true,
        stale_reattach: true,
        ..State::default()
    }));
    let connection = backend(stale.clone()).connect().await.unwrap();

    assert!(!connection.reattached);
    assert_eq!(stale.lock().unwrap().reattachments, 1);
    assert_eq!(stale.lock().unwrap().launches, 1);

    let rejected = Arc::new(Mutex::new(State {
        engine_id: Some(Uuid::from_u128(2)),
        engine_healthy: true,
        sidecar_healthy: true,
        reject_reattach: true,
        ..State::default()
    }));
    let mut lifecycle = FoundationLifecycle::new(backend(rejected.clone()));
    assert_eq!(lifecycle.setup().await, Err(Component::Engine));
    assert_eq!(lifecycle.status().state, "degraded");
    assert_eq!(lifecycle.status().engine, "failed");
    assert_eq!(lifecycle.status().sidecar, "ready");
    assert_eq!(rejected.lock().unwrap().reattachments, 1);
    assert_eq!(rejected.lock().unwrap().launches, 0);
}

#[tokio::test]
async fn retry_after_failed_setup_restores_health_polling_and_engine_identity() {
    let state = Arc::new(Mutex::new(State {
        reject_launch: true,
        ..State::default()
    }));
    let shell = ShellLifecycle::new(backend(state.clone()));
    assert_eq!(shell.setup().await, Err(Component::Engine));
    state.lock().unwrap().reject_launch = false;
    assert_eq!(shell.retry(Component::Engine).await.unwrap().state, "ready");

    state.lock().unwrap().sidecar_healthy = false;
    let status = shell.status().await;
    assert_eq!(status.sidecar, "failed");
    assert_eq!(status.engine, "ready");
    assert_eq!(status.state, "degraded");
    assert_eq!(shell.engine_id().await, state.lock().unwrap().engine_id);
    shell.shutdown().await.unwrap();
}

#[tokio::test]
async fn production_retry_preserves_engine_for_sidecar_and_replaces_engine_for_engine() {
    let state = Arc::new(Mutex::new(State::default()));
    let mut backend = backend(state.clone());
    let original = backend.connect().await.unwrap().engine_id;

    backend.retry(Component::Sidecar).await.unwrap();
    {
        let state = state.lock().unwrap();
        assert_eq!(state.engine_id, Some(original));
        assert_eq!(state.launches, 1);
        assert_eq!(state.reattachments, 0);
        assert_eq!(state.sidecar_restarts, 1);
        assert_eq!(state.shutdowns, 0);
    }

    backend.retry(Component::Engine).await.unwrap();
    let state = state.lock().unwrap();
    assert_ne!(state.engine_id, Some(original));
    assert_eq!(state.launches, 1);
    assert_eq!(state.reattachments, 0);
    assert_eq!(state.sidecar_restarts, 1);
    assert_eq!(state.engine_restarts, 1);
    assert_eq!(state.shutdowns, 0);
}

#[tokio::test]
async fn failed_production_retry_retains_the_unrelated_healthy_component() {
    let sidecar_state = Arc::new(Mutex::new(State::default()));
    let mut sidecar = FoundationLifecycle::new(backend(sidecar_state.clone()));
    sidecar.setup().await.unwrap();
    let engine_id = sidecar_state.lock().unwrap().engine_id;
    sidecar_state.lock().unwrap().sidecar_healthy = false;
    sidecar.refresh().await;
    sidecar_state.lock().unwrap().reject_reattach = true;

    assert_eq!(
        sidecar.retry(Component::Sidecar).await,
        Err(Component::Sidecar)
    );
    assert_eq!(sidecar.status().engine, "ready");
    assert_eq!(sidecar.status().sidecar, "failed");
    sidecar.refresh().await;
    assert_eq!(sidecar.status().engine, "ready");
    assert_eq!(sidecar.status().sidecar, "failed");
    sidecar_state.lock().unwrap().reject_reattach = false;
    sidecar.retry(Component::Sidecar).await.unwrap();
    assert_eq!(sidecar_state.lock().unwrap().engine_id, engine_id);

    let engine_state = Arc::new(Mutex::new(State::default()));
    let mut engine = FoundationLifecycle::new(backend(engine_state.clone()));
    engine.setup().await.unwrap();
    engine_state.lock().unwrap().engine_healthy = false;
    engine.refresh().await;
    engine_state.lock().unwrap().reject_launch = true;

    assert_eq!(
        engine.retry(Component::Engine).await,
        Err(Component::Engine)
    );
    assert_eq!(engine.status().engine, "failed");
    assert_eq!(engine.status().sidecar, "ready");
    assert!(engine_state.lock().unwrap().sidecar_healthy);
    engine.refresh().await;
    assert_eq!(engine.status().engine, "failed");
    assert_eq!(engine.status().sidecar, "ready");
    engine_state.lock().unwrap().sidecar_healthy = false;
    engine.refresh().await;
    assert_eq!(engine.status().sidecar, "failed");
    engine_state.lock().unwrap().reject_launch = false;
    engine.retry(Component::Engine).await.unwrap();
    assert_eq!(engine.status().engine, "ready");
    assert_eq!(engine.status().sidecar, "failed");
}
