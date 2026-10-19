#![allow(async_fn_in_trait)]

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use autodj_live_supervisor::{
    ChildCommand, LaunchConfig, LiveSession, LiveSupervisor, RuntimeRecord, RuntimeStore,
    SupervisorError,
};
use serde::{Deserialize, Serialize};
use tauri::{Manager, RunEvent};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Component {
    Engine,
    Sidecar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackendHealth {
    pub engine: bool,
    pub sidecar: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackendConnection {
    pub engine_id: Uuid,
    pub reattached: bool,
    pub health: BackendHealth,
}

pub trait SupervisorBackend: Send {
    async fn connect(&mut self) -> Result<BackendConnection, Component>;
    async fn health(&mut self) -> Result<BackendHealth, Component>;
    async fn retry(&mut self, component: Component) -> Result<BackendHealth, Component>;
    async fn shutdown(&mut self) -> Result<(), Component>;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationStatus {
    pub engine: String,
    pub sidecar: String,
    pub state: String,
    pub uptime_seconds: u64,
}

impl FoundationStatus {
    pub fn starting() -> Self {
        Self {
            engine: "starting".into(),
            sidecar: "starting".into(),
            state: "starting".into(),
            uptime_seconds: 0,
        }
    }

    fn from_health(health: BackendHealth) -> Self {
        let state = if health.engine && health.sidecar {
            "ready"
        } else {
            "degraded"
        };
        Self {
            engine: if health.engine { "ready" } else { "failed" }.into(),
            sidecar: if health.sidecar { "ready" } else { "failed" }.into(),
            state: state.into(),
            uptime_seconds: 0,
        }
    }

    fn failure(component: Component) -> Self {
        let mut status = Self::from_health(BackendHealth {
            engine: true,
            sidecar: true,
        });
        match component {
            Component::Engine => status.engine = "failed".into(),
            Component::Sidecar => status.sidecar = "failed".into(),
        }
        status.state = "degraded".into();
        status
    }
}

pub struct FoundationLifecycle<B> {
    backend: B,
    status: FoundationStatus,
    engine_id: Option<Uuid>,
    started: Instant,
    setup_complete: bool,
    reattached: bool,
    stopped: bool,
    pending_transition: Option<FoundationStatus>,
}

impl<B: SupervisorBackend> FoundationLifecycle<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            status: FoundationStatus::starting(),
            engine_id: None,
            started: Instant::now(),
            setup_complete: false,
            reattached: false,
            stopped: false,
            pending_transition: None,
        }
    }

    pub async fn setup(&mut self) -> Result<(), Component> {
        if self.setup_complete {
            return Ok(());
        }
        match self.backend.connect().await {
            Ok(connection) => {
                self.engine_id = Some(connection.engine_id);
                self.reattached = connection.reattached;
                let ready = FoundationStatus::from_health(connection.health);
                if connection.reattached {
                    self.status = FoundationStatus {
                        engine: "reattaching".into(),
                        sidecar: "reattaching".into(),
                        state: "reattaching".into(),
                        uptime_seconds: 0,
                    };
                    self.pending_transition = Some(ready);
                } else {
                    self.status = ready;
                }
                self.setup_complete = true;
                Ok(())
            }
            Err(component) => {
                self.status = FoundationStatus::failure(component);
                Err(component)
            }
        }
    }

    pub async fn refresh(&mut self) {
        if !self.setup_complete || self.stopped {
            return;
        }
        self.status = match self.backend.health().await {
            Ok(health) => FoundationStatus::from_health(health),
            Err(component) => FoundationStatus::failure(component),
        };
    }

    pub async fn retry(&mut self, component: Component) -> Result<(), Component> {
        self.status = FoundationStatus::from_health(self.backend.retry(component).await?);
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<(), Component> {
        if self.stopped {
            return Ok(());
        }
        self.backend.shutdown().await?;
        self.stopped = true;
        self.status = FoundationStatus {
            engine: "stopped".into(),
            sidecar: "stopped".into(),
            state: "stopped".into(),
            uptime_seconds: self.started.elapsed().as_secs(),
        };
        Ok(())
    }

    pub fn status(&self) -> FoundationStatus {
        let mut status = self.status.clone();
        status.uptime_seconds = self.started.elapsed().as_secs();
        status
    }

    pub fn engine_id(&self) -> Option<Uuid> {
        self.engine_id
    }

    pub fn was_reattached(&self) -> bool {
        self.reattached
    }

    pub fn poll_transition(&mut self) -> Option<FoundationStatus> {
        let pending = self.pending_transition.take()?;
        let current = self.status();
        self.status = pending;
        Some(current)
    }
}

struct ProductionSupervisor {
    config: LaunchConfig,
    session: Option<LiveSession>,
}

impl ProductionSupervisor {
    fn new(resource_dir: PathBuf) -> Result<Self, SupervisorError> {
        let engine = std::env::var_os("AUTODJ_LIVE_ENGINE")
            .map(PathBuf::from)
            .unwrap_or_else(|| resource_dir.join("autodj-live-engine.exe"));
        let python = std::env::var_os("AUTODJ_LIVE_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| resource_dir.join("python").join("python.exe"));
        Ok(Self {
            config: LaunchConfig {
                engine: ChildCommand::new(engine),
                sidecar: ChildCommand::bundled_python(python),
                runtime: RuntimeStore::installed()?,
            },
            session: None,
        })
    }

    async fn reconnect(&mut self) -> Result<BackendConnection, Component> {
        let existing = RuntimeRecord::load(self.config.clone());
        let (session, reattached) = match existing {
            Ok(record) => (
                LiveSupervisor::reattach(record)
                    .await
                    .map_err(|_| Component::Engine)?,
                true,
            ),
            Err(SupervisorError::StaleRecord) => (
                LiveSupervisor::launch(self.config.clone())
                    .await
                    .map_err(|_| Component::Engine)?,
                false,
            ),
            Err(_) => return Err(Component::Engine),
        };
        let engine_id = session.identity().engine_id;
        self.session = Some(session);
        let health = self.health().await?;
        Ok(BackendConnection {
            engine_id,
            reattached,
            health,
        })
    }
}

impl SupervisorBackend for ProductionSupervisor {
    async fn connect(&mut self) -> Result<BackendConnection, Component> {
        self.reconnect().await
    }

    async fn health(&mut self) -> Result<BackendHealth, Component> {
        let session = self.session.as_mut().ok_or(Component::Engine)?;
        session.health().await.map_err(|_| Component::Engine)?;
        Ok(BackendHealth {
            engine: true,
            sidecar: session.sidecar_healthy().await.unwrap_or(false),
        })
    }

    async fn retry(&mut self, component: Component) -> Result<BackendHealth, Component> {
        // Detach establishes the orphaned state before reconnecting, so a
        // sidecar retry preserves the engine without racing a second owner.
        if let Some(session) = self.session.take() {
            session.detach().await.map_err(|_| component)?;
        }
        self.reconnect()
            .await
            .map(|connection| connection.health)
            .map_err(|_| component)
    }

    async fn shutdown(&mut self) -> Result<(), Component> {
        if let Some(session) = self.session.take() {
            session.shutdown().await.map_err(|_| Component::Engine)?;
        }
        Ok(())
    }
}

struct AppState {
    lifecycle: Mutex<FoundationLifecycle<ProductionSupervisor>>,
    exiting: AtomicBool,
}

#[tauri::command]
async fn get_foundation_status(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<FoundationStatus, ()> {
    let mut lifecycle = state.lifecycle.lock().await;
    if let Some(status) = lifecycle.poll_transition() {
        return Ok(status);
    }
    lifecycle.refresh().await;
    Ok(lifecycle.status())
}

#[tauri::command]
async fn retry_component(
    component: Component,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<FoundationStatus, ()> {
    let mut lifecycle = state.lifecycle.lock().await;
    lifecycle.retry(component).await.map_err(|_| ())?;
    Ok(lifecycle.status())
}

#[tauri::command]
async fn shutdown_live(state: tauri::State<'_, Arc<AppState>>) -> Result<(), ()> {
    state
        .lifecycle
        .lock()
        .await
        .shutdown()
        .await
        .map_err(|_| ())
}

pub fn run() {
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_foundation_status,
            retry_component,
            shutdown_live
        ])
        .setup(|app| {
            let resource_dir = app.path().resource_dir()?;
            let backend = ProductionSupervisor::new(resource_dir)?;
            app.manage(Arc::new(AppState {
                lifecycle: Mutex::new(FoundationLifecycle::new(backend)),
                exiting: AtomicBool::new(false),
            }));
            let state = app.state::<Arc<AppState>>().inner().clone();
            tauri::async_runtime::spawn(async move {
                let _ = state.lifecycle.lock().await.setup().await;
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build AutoDJ Live");

    app.run(|handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            let state = handle.state::<Arc<AppState>>().inner().clone();
            if !state.exiting.swap(true, Ordering::SeqCst) {
                api.prevent_exit();
                let handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = state.lifecycle.lock().await.shutdown().await;
                    handle.exit(0);
                });
            }
        }
    });
}
