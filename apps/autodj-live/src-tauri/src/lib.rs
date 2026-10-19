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
    fn connect(
        &mut self,
    ) -> impl std::future::Future<Output = Result<BackendConnection, Component>> + Send;
    fn health(
        &mut self,
    ) -> impl std::future::Future<Output = Result<BackendHealth, Component>> + Send;
    fn retry(
        &mut self,
        component: Component,
    ) -> impl std::future::Future<Output = Result<BackendHealth, Component>> + Send;
    fn shutdown(&mut self) -> impl std::future::Future<Output = Result<(), Component>> + Send;
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
        match self.backend.retry(component).await {
            Ok(health) => {
                self.status = FoundationStatus::from_health(health);
                Ok(())
            }
            Err(component) => {
                self.refresh().await;
                Err(component)
            }
        }
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

pub struct ShellLifecycle<B> {
    lifecycle: Mutex<FoundationLifecycle<B>>,
    exiting: AtomicBool,
}

impl<B: SupervisorBackend> ShellLifecycle<B> {
    pub fn new(backend: B) -> Self {
        Self {
            lifecycle: Mutex::new(FoundationLifecycle::new(backend)),
            exiting: AtomicBool::new(false),
        }
    }

    pub async fn setup(&self) -> Result<(), Component> {
        self.lifecycle.lock().await.setup().await
    }

    pub async fn status(&self) -> FoundationStatus {
        let mut lifecycle = self.lifecycle.lock().await;
        if let Some(status) = lifecycle.poll_transition() {
            return status;
        }
        lifecycle.refresh().await;
        lifecycle.status()
    }

    pub async fn retry(&self, component: Component) -> Result<FoundationStatus, Component> {
        let mut lifecycle = self.lifecycle.lock().await;
        lifecycle.retry(component).await?;
        Ok(lifecycle.status())
    }

    pub async fn shutdown(&self) -> Result<(), Component> {
        self.lifecycle.lock().await.shutdown().await
    }

    pub fn claim_exit(&self) -> bool {
        !self.exiting.swap(true, Ordering::SeqCst)
    }

    pub async fn shutdown_after_exit_request(&self) {
        let _ = self.shutdown().await;
    }

    pub async fn engine_id(&self) -> Option<Uuid> {
        self.lifecycle.lock().await.engine_id()
    }
}

pub fn request_orderly_exit<B, P, F>(state: Arc<ShellLifecycle<B>>, prevent: P, exit: F) -> bool
where
    B: SupervisorBackend + 'static,
    P: FnOnce(),
    F: FnOnce() + Send + 'static,
{
    if !state.claim_exit() {
        return false;
    }
    prevent();
    tauri::async_runtime::spawn(async move {
        state.shutdown_after_exit_request().await;
        exit();
    });
    true
}

pub trait SupervisorSession: Send {
    fn engine_id(&self) -> Uuid;
    fn engine_healthy(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), SupervisorError>> + Send;
    fn sidecar_healthy(
        &self,
    ) -> impl std::future::Future<Output = Result<bool, SupervisorError>> + Send;
    fn restart_engine(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), SupervisorError>> + Send;
    fn restart_sidecar(
        &mut self,
    ) -> impl std::future::Future<Output = Result<(), SupervisorError>> + Send;
    fn shutdown(self) -> impl std::future::Future<Output = Result<(), SupervisorError>> + Send;
}

impl SupervisorSession for LiveSession {
    fn engine_id(&self) -> Uuid {
        self.identity().engine_id
    }

    async fn engine_healthy(&mut self) -> Result<(), SupervisorError> {
        LiveSession::health(self).await.map(|_| ())
    }

    async fn sidecar_healthy(&self) -> Result<bool, SupervisorError> {
        LiveSession::sidecar_healthy(self).await
    }

    async fn restart_engine(&mut self) -> Result<(), SupervisorError> {
        LiveSession::restart_engine(self).await
    }

    async fn restart_sidecar(&mut self) -> Result<(), SupervisorError> {
        LiveSession::restart_sidecar(self).await
    }

    async fn shutdown(self) -> Result<(), SupervisorError> {
        LiveSession::shutdown(self).await
    }
}

pub trait SupervisorRuntime: Send {
    type Record: Send;
    type Session: SupervisorSession;

    fn load(&mut self, config: LaunchConfig) -> Result<Self::Record, SupervisorError>;
    fn reattach(
        &mut self,
        record: Self::Record,
    ) -> impl std::future::Future<Output = Result<Self::Session, SupervisorError>> + Send;
    fn launch(
        &mut self,
        config: LaunchConfig,
    ) -> impl std::future::Future<Output = Result<Self::Session, SupervisorError>> + Send;
}

pub struct NativeSupervisorRuntime;

impl SupervisorRuntime for NativeSupervisorRuntime {
    type Record = RuntimeRecord;
    type Session = LiveSession;

    fn load(&mut self, config: LaunchConfig) -> Result<Self::Record, SupervisorError> {
        RuntimeRecord::load(config)
    }

    async fn reattach(&mut self, record: Self::Record) -> Result<Self::Session, SupervisorError> {
        LiveSupervisor::reattach(record).await
    }

    async fn launch(&mut self, config: LaunchConfig) -> Result<Self::Session, SupervisorError> {
        LiveSupervisor::launch(config).await
    }
}

pub struct ProductionSupervisor<R: SupervisorRuntime = NativeSupervisorRuntime> {
    config: LaunchConfig,
    session: Option<R::Session>,
    runtime: R,
    engine_id: Option<Uuid>,
}

pub fn resolve_child_paths(
    resource_dir: PathBuf,
    release: bool,
    engine_override: Option<PathBuf>,
    python_override: Option<PathBuf>,
) -> (PathBuf, PathBuf) {
    let bundled_engine = resource_dir.join("autodj-live-engine.exe");
    let bundled_python = resource_dir.join("python").join("python.exe");
    if release {
        (bundled_engine, bundled_python)
    } else {
        (
            engine_override.unwrap_or(bundled_engine),
            python_override.unwrap_or(bundled_python),
        )
    }
}

impl ProductionSupervisor<NativeSupervisorRuntime> {
    fn new(resource_dir: PathBuf) -> Result<Self, SupervisorError> {
        let release = env!("AUTODJ_BUILD_PROFILE") == "release";
        let engine_override = (!release)
            .then(|| std::env::var_os("AUTODJ_LIVE_ENGINE").map(PathBuf::from))
            .flatten();
        let python_override = (!release)
            .then(|| std::env::var_os("AUTODJ_LIVE_PYTHON").map(PathBuf::from))
            .flatten();
        let (engine, python) =
            resolve_child_paths(resource_dir, release, engine_override, python_override);
        Ok(Self {
            config: LaunchConfig {
                engine: ChildCommand::new(engine),
                sidecar: ChildCommand::bundled_python(python),
                runtime: RuntimeStore::installed()?,
            },
            session: None,
            runtime: NativeSupervisorRuntime,
            engine_id: None,
        })
    }
}

impl<R: SupervisorRuntime> ProductionSupervisor<R> {
    pub fn with_runtime(config: LaunchConfig, runtime: R) -> Self {
        Self {
            config,
            session: None,
            runtime,
            engine_id: None,
        }
    }

    async fn reconnect(&mut self) -> Result<BackendConnection, SupervisorError> {
        let existing = self.runtime.load(self.config.clone());
        let (session, reattached) = match existing {
            Ok(record) => match self.runtime.reattach(record).await {
                Ok(session) => (session, true),
                Err(SupervisorError::StaleRecord) => {
                    (self.runtime.launch(self.config.clone()).await?, false)
                }
                Err(error) => return Err(error),
            },
            Err(SupervisorError::StaleRecord) => {
                (self.runtime.launch(self.config.clone()).await?, false)
            }
            Err(error) => return Err(error),
        };
        self.accept(session, reattached).await
    }

    async fn accept(
        &mut self,
        session: R::Session,
        reattached: bool,
    ) -> Result<BackendConnection, SupervisorError> {
        let engine_id = session.engine_id();
        self.session = Some(session);
        let health = self.health_snapshot().await?;
        self.engine_id = Some(engine_id);
        Ok(BackendConnection {
            engine_id,
            reattached,
            health,
        })
    }

    async fn health_snapshot(&mut self) -> Result<BackendHealth, SupervisorError> {
        let session = self.session.as_mut().ok_or(SupervisorError::StaleRecord)?;
        Ok(BackendHealth {
            engine: session.engine_healthy().await.is_ok(),
            sidecar: session.sidecar_healthy().await.unwrap_or(false),
        })
    }

    async fn retry_inner(&mut self, component: Component) -> Result<BackendHealth, Component> {
        let Some(session) = self.session.as_mut() else {
            return self
                .reconnect()
                .await
                .map(|connection| connection.health)
                .map_err(|_| component);
        };
        match component {
            Component::Sidecar => {
                session.restart_sidecar().await.map_err(|_| component)?;
            }
            Component::Engine => {
                session.restart_engine().await.map_err(|_| component)?;
            }
        }
        self.engine_id = Some(session.engine_id());
        self.health_snapshot().await.map_err(|_| component)
    }
}

impl<R: SupervisorRuntime> SupervisorBackend for ProductionSupervisor<R> {
    async fn connect(&mut self) -> Result<BackendConnection, Component> {
        self.reconnect().await.map_err(|_| Component::Engine)
    }

    async fn health(&mut self) -> Result<BackendHealth, Component> {
        self.health_snapshot().await.map_err(|_| Component::Engine)
    }

    async fn retry(&mut self, component: Component) -> Result<BackendHealth, Component> {
        self.retry_inner(component).await
    }

    async fn shutdown(&mut self) -> Result<(), Component> {
        if let Some(session) = self.session.take() {
            session.shutdown().await.map_err(|_| Component::Engine)?;
        }
        Ok(())
    }
}

type AppState = ShellLifecycle<ProductionSupervisor>;

#[tauri::command]
async fn get_foundation_status(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<FoundationStatus, ()> {
    Ok(state.status().await)
}

#[tauri::command]
async fn retry_component(
    component: Component,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<FoundationStatus, ()> {
    state.retry(component).await.map_err(|_| ())
}

#[tauri::command]
async fn shutdown_live(state: tauri::State<'_, Arc<AppState>>) -> Result<(), ()> {
    state.shutdown().await.map_err(|_| ())
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
            app.manage(Arc::new(ShellLifecycle::new(backend)));
            let state = app.state::<Arc<AppState>>().inner().clone();
            tauri::async_runtime::spawn(async move {
                let _ = state.setup().await;
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build AutoDJ Live");

    app.run(|handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            let state = handle.state::<Arc<AppState>>().inner().clone();
            let handle = handle.clone();
            request_orderly_exit(state, || api.prevent_exit(), move || handle.exit(0));
        }
    });
}
