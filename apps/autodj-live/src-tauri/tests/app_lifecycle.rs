use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use autodj_live_app::{
    AsioRegistration, AudioDirection, AudioDiscovery, AudioEndpoint, AudioEndpointState,
    BackendConnection, BackendHealth, Component, FoundationLifecycle, ShellLifecycle,
    SupervisorBackend, WasapiCapabilities, request_orderly_exit,
};
use uuid::Uuid;

#[derive(Default)]
struct Shared {
    active_engine: Option<Uuid>,
    launches: usize,
    shutdowns: usize,
    retries: Vec<Component>,
    discovery_requests: usize,
    probe_requests: Vec<String>,
}

#[derive(Clone)]
struct FakeSupervisor {
    shared: Arc<Mutex<Shared>>,
    health: BackendHealth,
}

impl FakeSupervisor {
    fn new(shared: Arc<Mutex<Shared>>) -> Self {
        Self {
            shared,
            health: BackendHealth {
                engine: true,
                sidecar: true,
            },
        }
    }
}

impl SupervisorBackend for FakeSupervisor {
    async fn connect(&mut self) -> Result<BackendConnection, Component> {
        let mut shared = self.shared.lock().unwrap();
        let reattached = shared.active_engine.is_some();
        let engine_id = *shared.active_engine.get_or_insert_with(Uuid::new_v4);
        shared.launches += usize::from(!reattached);
        Ok(BackendConnection {
            engine_id,
            reattached,
            health: self.health,
        })
    }

    async fn health(&mut self) -> Result<BackendHealth, Component> {
        Ok(self.health)
    }

    async fn retry(&mut self, component: Component) -> Result<BackendHealth, Component> {
        self.shared.lock().unwrap().retries.push(component);
        match component {
            Component::Engine => self.health.engine = true,
            Component::Sidecar => self.health.sidecar = true,
        }
        Ok(self.health)
    }

    async fn shutdown(&mut self) -> Result<(), Component> {
        let mut shared = self.shared.lock().unwrap();
        shared.shutdowns += 1;
        shared.active_engine = None;
        Ok(())
    }

    async fn audio_discovery(&mut self) -> Result<AudioDiscovery, Component> {
        self.shared.lock().unwrap().discovery_requests += 1;
        Ok(AudioDiscovery {
            endpoints: vec![AudioEndpoint {
                id: "rane-output".into(),
                name: "Deck 1 L/R".into(),
                direction: AudioDirection::Output,
                state: AudioEndpointState::Active,
            }],
            asio_drivers: vec![AsioRegistration {
                registration: "Rane PERFORMER ASIO".into(),
                clsid: "{6e9aac96-aeda-4f9a-b222-13ea83facee8}".into(),
            }],
            issues: vec![],
        })
    }

    async fn probe_wasapi(&mut self, endpoint_id: &str) -> Result<WasapiCapabilities, Component> {
        self.shared
            .lock()
            .unwrap()
            .probe_requests
            .push(endpoint_id.into());
        Ok(WasapiCapabilities {
            endpoint_id: endpoint_id.into(),
            channels: 8,
            sample_rate_hz: 48_000,
            bits_per_sample: 32,
            default_period_100ns: 100_000,
            minimum_period_100ns: 30_000,
            exclusive_shared_mix_supported: true,
            exclusive_shared_format_sample_rates_hz: Some(vec![44_100, 48_000]),
        })
    }
}

#[tokio::test]
async fn wasapi_probe_is_user_requested_and_passes_through_engine_data() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let shell = ShellLifecycle::new(FakeSupervisor::new(shared.clone()));
    shell.setup().await.unwrap();
    assert!(shared.lock().unwrap().probe_requests.is_empty());

    let capabilities = shell.probe_wasapi("rane-output").await.unwrap();

    assert_eq!(shared.lock().unwrap().probe_requests, ["rane-output"]);
    assert_eq!(capabilities.channels, 8);
    assert_eq!(capabilities.sample_rate_hz, 48_000);
}

#[tokio::test]
async fn audio_discovery_is_user_requested_and_passes_through_engine_data() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let shell = ShellLifecycle::new(FakeSupervisor::new(shared.clone()));
    shell.setup().await.unwrap();
    assert_eq!(shared.lock().unwrap().discovery_requests, 0);

    let discovery = shell.audio_discovery().await.unwrap();

    assert_eq!(shared.lock().unwrap().discovery_requests, 1);
    assert_eq!(discovery.endpoints[0].id, "rane-output");
    assert_eq!(
        discovery.asio_drivers[0].registration,
        "Rane PERFORMER ASIO"
    );
}

#[tokio::test]
async fn shell_setup_launches_once_and_a_second_window_does_not_spawn_children() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let shell = ShellLifecycle::new(FakeSupervisor::new(shared.clone()));

    shell.setup().await.unwrap();
    shell.setup().await.unwrap();

    assert_eq!(shared.lock().unwrap().launches, 1);
}

#[tokio::test]
async fn webview_reload_observes_the_same_engine_identity() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let shell = ShellLifecycle::new(FakeSupervisor::new(shared));
    shell.setup().await.unwrap();

    let before = shell.engine_id().await.unwrap();
    shell.status().await;

    assert_eq!(shell.engine_id().await, Some(before));
}

#[tokio::test]
async fn shell_restart_reattaches_during_the_lease() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let mut first = FoundationLifecycle::new(FakeSupervisor::new(shared.clone()));
    first.setup().await.unwrap();
    let engine_id = first.engine_id().unwrap();
    drop(first);

    let mut restarted = FoundationLifecycle::new(FakeSupervisor::new(shared.clone()));
    restarted.setup().await.unwrap();

    assert_eq!(restarted.engine_id(), Some(engine_id));
    assert_eq!(shared.lock().unwrap().launches, 1);
    assert!(restarted.was_reattached());
    assert_eq!(restarted.status().state, "reattaching");
    assert_eq!(restarted.poll_transition().unwrap().state, "reattaching");
    assert_eq!(restarted.status().state, "ready");
}

#[tokio::test]
async fn retry_targets_only_the_failed_component() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let mut fake = FakeSupervisor::new(shared.clone());
    fake.health.sidecar = false;
    let mut lifecycle = FoundationLifecycle::new(fake);
    lifecycle.setup().await.unwrap();

    lifecycle.retry(Component::Sidecar).await.unwrap();

    assert_eq!(shared.lock().unwrap().retries, vec![Component::Sidecar]);
}

#[tokio::test]
async fn normal_exit_requests_orderly_shutdown_once() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let shell = Arc::new(ShellLifecycle::new(FakeSupervisor::new(shared.clone())));
    shell.setup().await.unwrap();
    let prevented = Arc::new(AtomicBool::new(false));
    let exited = Arc::new(AtomicBool::new(false));
    let prevent_signal = prevented.clone();
    let exit_signal = exited.clone();

    assert!(request_orderly_exit(
        shell.clone(),
        move || prevent_signal.store(true, Ordering::SeqCst),
        move || exit_signal.store(true, Ordering::SeqCst),
    ));
    assert!(prevented.load(Ordering::SeqCst));
    while !exited.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
    }
    assert!(!request_orderly_exit(shell.clone(), || {}, || {}));

    assert_eq!(shared.lock().unwrap().shutdowns, 1);
    assert_eq!(shell.status().await.state, "stopped");
}

#[test]
fn public_status_dto_contains_no_process_or_transport_identifiers() {
    let fields: HashMap<String, serde_json::Value> = serde_json::from_value(
        serde_json::to_value(autodj_live_app::FoundationStatus::starting()).unwrap(),
    )
    .unwrap();

    assert_eq!(
        fields.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["engine", "sidecar", "state", "uptimeSeconds"])
    );
}

#[test]
fn bundle_resources_land_at_the_production_child_defaults() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let resources = config["bundle"]["resources"].as_object().unwrap();

    assert_eq!(
        resources["../../../target/x86_64-pc-windows-msvc/release/autodj-live-engine.exe"],
        "autodj-live-engine.exe"
    );
    assert_eq!(resources["../../../target/live-sidecar/python"], "python");
}
