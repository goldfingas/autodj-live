//! Windows process supervision for the AutoDJ Live foundation.
//!
//! Dropping a session disconnects its engine (the engine owns its 30-second
//! lease) and closes the sidecar's kill-on-close job. Call `shutdown` for normal
//! application exit. Credentials are intentionally absent from the public API.

mod discovery;
mod process;
mod session;
mod windows;

pub use discovery::{Protection, RuntimeRecord, RuntimeStore};
pub use process::ChildCommand;
pub use session::{LiveSession, LiveSupervisor, SessionIdentity};

#[derive(Clone)]
pub struct LaunchConfig {
    pub engine: ChildCommand,
    pub sidecar: ChildCommand,
    pub runtime: RuntimeStore,
}

/// Fixed messages only: OS, protocol, child output and credentials are never
/// interpolated into diagnostic text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SupervisorError {
    #[error("an absolute executable path is required")]
    InvalidExecutable,
    #[error("could not start child process")]
    Spawn,
    #[error("child startup timed out")]
    StartupTimeout,
    #[error("startup descriptor exceeds the size limit")]
    DescriptorTooLarge,
    #[error("invalid child startup descriptor")]
    InvalidDescriptor,
    #[error("could not establish private runtime storage")]
    Privacy,
    #[error("could not read or replace runtime discovery")]
    Discovery,
    #[error("another supervisor operation is in progress")]
    Busy,
    #[error("the engine has an active supervisor")]
    AlreadyRunning,
    #[error("no matching live engine is available")]
    StaleRecord,
    #[error("could not authenticate the expected child")]
    Authentication,
    #[error("engine control connection failed")]
    Connection,
    #[error("engine rejected the supervisor command")]
    Rejected,
    #[error("child exit was not confirmed")]
    ExitUnconfirmed,
    #[error("could not generate launch credentials")]
    Random,
}
