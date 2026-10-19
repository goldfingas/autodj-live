use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
};

use autodj_live_app::{
    BackendConnection, BackendHealth, Component, FoundationLifecycle, SupervisorBackend,
};
use uuid::Uuid;

#[derive(Default)]
struct Shared {
    active_engine: Option<Uuid>,
    launches: usize,
    shutdowns: usize,
    retries: Vec<Component>,
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
}

#[tokio::test]
async fn setup_launches_once_and_an_extra_window_does_not_spawn_children() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let mut lifecycle = FoundationLifecycle::new(FakeSupervisor::new(shared.clone()));

    lifecycle.setup().await.unwrap();
    lifecycle.setup().await.unwrap();

    assert_eq!(shared.lock().unwrap().launches, 1);
}

#[tokio::test]
async fn webview_reload_observes_the_same_engine_identity() {
    let shared = Arc::new(Mutex::new(Shared::default()));
    let mut lifecycle = FoundationLifecycle::new(FakeSupervisor::new(shared));
    lifecycle.setup().await.unwrap();

    let before = lifecycle.engine_id().unwrap();
    lifecycle.refresh().await;

    assert_eq!(lifecycle.engine_id(), Some(before));
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
    let mut lifecycle = FoundationLifecycle::new(FakeSupervisor::new(shared.clone()));
    lifecycle.setup().await.unwrap();

    lifecycle.shutdown().await.unwrap();
    lifecycle.shutdown().await.unwrap();

    assert_eq!(shared.lock().unwrap().shutdowns, 1);
    assert_eq!(lifecycle.status().state, "stopped");
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
