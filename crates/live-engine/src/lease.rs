use std::time::{Duration, Instant};

use autodj_live_protocol::{EngineCommand, EngineEvent, HealthSnapshot};
use tokio::sync::watch;
use uuid::Uuid;

pub struct Lease {
    engine_id: Uuid,
    started_ns: u64,
    reconnect: Duration,
    deadline: Option<Instant>,
    deadline_ns: u64,
    generation: u64,
    owner: Option<(u64, watch::Sender<bool>)>,
    pub stopping: bool,
}

impl Lease {
    pub fn new(engine_id: Uuid, reconnect: Duration) -> Self {
        let started = Instant::now();
        let started_ns = monotonic_ns();
        let deadline_ns =
            started_ns.saturating_add(reconnect.as_nanos().try_into().unwrap_or(u64::MAX));
        Self {
            engine_id,
            started_ns,
            reconnect,
            deadline: Some(started + reconnect),
            deadline_ns,
            generation: 0,
            owner: None,
            stopping: false,
        }
    }

    pub fn expired(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub fn health(&self) -> HealthSnapshot {
        HealthSnapshot {
            engine_id: self.engine_id,
            state: if self.stopping {
                "stopping"
            } else if self.owner.is_some() {
                "attached"
            } else {
                "orphaned"
            }
            .to_owned(),
            orphaned: self.owner.is_none(),
            supervisor_generation: self.generation,
            started_ns: self.started_ns,
            // Wire timestamps use Windows system uptime, shared across processes.
            // An attached owner has no lease deadline, represented by zero.
            lease_deadline_ns: self.deadline_ns,
            extensions: Default::default(),
        }
    }

    pub fn disconnect(&mut self, id: u64) {
        if self.owner.as_ref().is_some_and(|(owner, _)| *owner == id) {
            self.owner = None;
            self.deadline = Some(Instant::now() + self.reconnect);
            self.deadline_ns = monotonic_ns()
                .saturating_add(self.reconnect.as_nanos().try_into().unwrap_or(u64::MAX));
        }
    }

    pub fn command(
        &mut self,
        id: u64,
        replaced: &watch::Sender<bool>,
        command: EngineCommand,
    ) -> EngineEvent {
        if self.stopping || self.expired() {
            self.stopping = true;
            return EngineEvent::Stopping;
        }
        match command {
            EngineCommand::Health => EngineEvent::Health(self.health()),
            EngineCommand::DiscoverAudio
            | EngineCommand::ProbeWasapi { .. }
            | EngineCommand::InitializeWasapiShared { .. } => {
                match self.authorize_audio_operation(id) {
                    Ok(()) => error("audio operation must be dispatched by the engine server"),
                    Err(event) => event,
                }
            }
            EngineCommand::Attach {
                supervisor_generation,
            } => {
                if supervisor_generation <= self.generation {
                    return error("stale supervisor generation");
                }
                if let Some((owner, signal)) = self.owner.take()
                    && owner != id
                {
                    let _ = signal.send(true);
                }
                self.owner = Some((id, replaced.clone()));
                self.generation = supervisor_generation;
                self.deadline = None;
                self.deadline_ns = 0;
                EngineEvent::Attached
            }
            EngineCommand::Detach => {
                if !self.owner.as_ref().is_some_and(|(owner, _)| *owner == id) {
                    return error("detach requires the active supervisor");
                }
                self.disconnect(id);
                EngineEvent::Health(self.health())
            }
            EngineCommand::Shutdown => {
                if self.owner.as_ref().is_some_and(|(owner, _)| *owner != id) {
                    return error("shutdown requires the active supervisor");
                }
                self.stopping = true;
                EngineEvent::Stopping
            }
        }
    }

    pub fn authorize_audio_operation(&self, id: u64) -> Result<(), EngineEvent> {
        if self.owner.as_ref().is_some_and(|(owner, _)| *owner == id) {
            Ok(())
        } else {
            Err(error("audio operation requires the active supervisor"))
        }
    }
}

/// Windows uptime is monotonic and comparable by the separate supervisor.
/// The wire unit is nanoseconds; the source is measured in milliseconds.
fn monotonic_ns() -> u64 {
    // SAFETY: GetTickCount64 takes no pointers and has no caller preconditions.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
        .saturating_mul(1_000_000)
}

fn error(message: &str) -> EngineEvent {
    EngineEvent::Error {
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_active_owner_may_request_audio_operations() {
        let mut lease = Lease::new(Uuid::new_v4(), Duration::from_secs(30));
        let (owner_signal, _) = watch::channel(false);
        let (stranger_signal, _) = watch::channel(false);
        assert_eq!(
            lease.command(
                7,
                &owner_signal,
                EngineCommand::Attach {
                    supervisor_generation: 1
                }
            ),
            EngineEvent::Attached
        );
        assert!(lease.authorize_audio_operation(7).is_ok());
        assert!(lease.authorize_audio_operation(8).is_err());
        assert_eq!(
            lease.command(8, &stranger_signal, EngineCommand::DiscoverAudio),
            EngineEvent::Error {
                message: "audio operation requires the active supervisor".into()
            }
        );
        assert_eq!(
            lease.command(
                8,
                &stranger_signal,
                EngineCommand::ProbeWasapi {
                    endpoint_id: "endpoint-1".into()
                }
            ),
            EngineEvent::Error {
                message: "audio operation requires the active supervisor".into()
            }
        );
        assert_eq!(
            lease.command(
                8,
                &stranger_signal,
                EngineCommand::InitializeWasapiShared {
                    endpoint_id: "endpoint-1".into()
                }
            ),
            EngineEvent::Error {
                message: "audio operation requires the active supervisor".into()
            }
        );
    }
}
