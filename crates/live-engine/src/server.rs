use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use autodj_live_protocol::{
    AsioRegistration, AudioDirection, AudioDiscovery, AudioEndpoint, AudioEndpointState,
    ClientHello, EngineCommand, EngineEvent, FrameCodec, ProtocolVersion, ServerHello,
};
use subtle::ConstantTimeEq;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, oneshot, watch},
    task::JoinSet,
    time::{interval, timeout},
};
use uuid::Uuid;

use crate::{
    lease::Lease,
    startup::{self, Config},
};

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const DISCOVERY_TIMEOUT: Duration = Duration::from_millis(1_500);
const MAX_CLIENTS: usize = 64;

#[derive(Clone, Default)]
struct DiscoveryGate(Arc<AtomicBool>);

struct DiscoveryGuard(Arc<AtomicBool>);

impl Drop for DiscoveryGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl DiscoveryGate {
    fn start<F>(
        &self,
        operation: F,
    ) -> Result<oneshot::Receiver<live_audio::discovery::Discovery>, ()>
    where
        F: FnOnce() -> live_audio::discovery::Discovery + Send + 'static,
    {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ())?;
        let active = self.0.clone();
        let (result, receiver) = oneshot::channel();
        let spawned = std::thread::Builder::new()
            .name("live-audio-discovery-request".into())
            .spawn(move || {
                let guard = DiscoveryGuard(active);
                let discovery = operation();
                drop(guard);
                let _ = result.send(discovery);
            });
        if spawned.is_err() {
            self.0.store(false, Ordering::Release);
            return Err(());
        }
        Ok(receiver)
    }
}

pub async fn run(config: Config) -> Result<(), &'static str> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "could not bind loopback listener")?;
    let engine_id = Uuid::new_v4();
    let port = listener
        .local_addr()
        .map_err(|_| "could not inspect loopback listener")?
        .port();
    startup::emit_descriptor(port, engine_id)?;
    let state = Arc::new(Mutex::new(Lease::new(engine_id, config.reconnect)));
    let discovery = DiscoveryGate::default();
    let config = Arc::new(config);
    let (shutdown, _) = watch::channel(false);
    let mut clients = JoinSet::new();
    let mut tick = interval(Duration::from_millis(5));
    let mut next_id = 0_u64;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let mut lease = state.lock().await;
                if lease.stopping || lease.expired() {
                    lease.stopping = true;
                    break;
                }
            }
            Some(_) = clients.join_next(), if !clients.is_empty() => {}
            accepted = listener.accept() => {
                let (socket, _) = accepted.map_err(|_| "loopback accept failed")?;
                if clients.len() >= MAX_CLIENTS { continue; }
                next_id = next_id.checked_add(1).ok_or("connection identifier exhausted")?;
                clients.spawn(serve(socket, next_id, engine_id, config.clone(), state.clone(), discovery.clone(), shutdown.subscribe()));
            }
        }
    }
    drop(listener);
    let _ = shutdown.send(true);
    // Every client write is bounded, including the final stopping notification.
    while clients.join_next().await.is_some() {}
    Ok(())
}

async fn serve(
    mut socket: TcpStream,
    id: u64,
    engine_id: Uuid,
    config: Arc<Config>,
    state: Arc<Mutex<Lease>>,
    discovery: DiscoveryGate,
    mut shutdown: watch::Receiver<bool>,
) {
    let hello = tokio::select! {
        _ = shutdown.changed() => return,
        result = timeout(IO_TIMEOUT, FrameCodec::read_typed::<_, ClientHello>(&mut socket)) => {
            match result { Ok(Ok(hello)) => hello, _ => return }
        }
    };
    // Evaluate both fixed-size credential comparisons without prefix-dependent equality.
    let valid = hello.token.as_bytes().ct_eq(config.token.as_bytes())
        & hello
            .session_id
            .as_bytes()
            .ct_eq(config.session_id.as_bytes());
    if !bool::from(valid) || hello.version.ensure_compatible().is_err() {
        return;
    }
    let deadline = {
        let lease = state.lock().await;
        if lease.stopping || lease.expired() {
            return;
        }
        lease.health().lease_deadline_ns
    };
    let response = ServerHello {
        version: ProtocolVersion::CURRENT,
        engine_id,
        pid: std::process::id(),
        lease_deadline_ns: deadline,
    };
    if !send(&mut socket, &response).await {
        return;
    }
    let (replaced, mut replacement) = watch::channel(false);
    loop {
        let event = tokio::select! {
            biased;
            _ = shutdown.changed() => EngineEvent::Stopping,
            _ = replacement.changed() => {
                let _ = send(&mut socket, &EngineEvent::Error { message: "supervisor replaced".to_owned() }).await;
                break;
            }
            command = FrameCodec::read(&mut socket) => {
                let Ok(command) = command else { break; };
                let mut lease = state.lock().await;
                // Replacement may race a ready command; a revoked socket cannot reattach.
                if *replacement.borrow() { break; }
                if matches!(command, EngineCommand::DiscoverAudio) {
                    match lease.authorize_audio_discovery(id) {
                        Err(event) => event,
                        Ok(()) => {
                            drop(lease);
                            let discovered = match discovery.start(live_audio::discovery::discover) {
                                Ok(receiver) => Some(timeout(DISCOVERY_TIMEOUT, receiver).await),
                                Err(()) => None,
                            };
                            let still_owner = state.lock().await.authorize_audio_discovery(id);
                            match (discovered, still_owner) {
                                (Some(Ok(Ok(discovery))), Ok(())) => EngineEvent::AudioDiscovery(map_discovery(discovery)),
                                (None, Ok(())) => EngineEvent::Error { message: "audio discovery already in progress".into() },
                                (_, Err(event)) => event,
                                _ => EngineEvent::Error { message: "audio discovery failed".into() },
                            }
                        }
                    }
                } else {
                    lease.command(id, &replaced, command)
                }
            }
        };
        let stopping = matches!(event, EngineEvent::Stopping);
        if !send(&mut socket, &event).await || stopping {
            break;
        }
    }
    state.lock().await.disconnect(id);
}

fn map_discovery(discovery: live_audio::discovery::Discovery) -> AudioDiscovery {
    AudioDiscovery {
        endpoints: discovery
            .endpoints
            .into_iter()
            .map(|endpoint| AudioEndpoint {
                id: endpoint.id,
                name: endpoint.name,
                direction: match endpoint.direction {
                    live_audio::discovery::Direction::Input => AudioDirection::Input,
                    live_audio::discovery::Direction::Output => AudioDirection::Output,
                },
                state: match endpoint.state {
                    live_audio::discovery::EndpointState::Active => AudioEndpointState::Active,
                    live_audio::discovery::EndpointState::Disabled => AudioEndpointState::Disabled,
                    live_audio::discovery::EndpointState::NotPresent => {
                        AudioEndpointState::NotPresent
                    }
                    live_audio::discovery::EndpointState::Unplugged => {
                        AudioEndpointState::Unplugged
                    }
                    live_audio::discovery::EndpointState::Unknown(value) => {
                        AudioEndpointState::Unknown(value)
                    }
                },
            })
            .collect(),
        asio_drivers: discovery
            .asio_drivers
            .into_iter()
            .map(|driver| AsioRegistration {
                registration: driver.registration,
                clsid: driver.clsid,
            })
            .collect(),
        issues: discovery.issues,
    }
}

async fn send<T: serde::Serialize>(socket: &mut TcpStream, message: &T) -> bool {
    matches!(
        timeout(IO_TIMEOUT, FrameCodec::write(socket, message)).await,
        Ok(Ok(()))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_worker_is_single_flight_and_does_not_own_runtime_shutdown() {
        let gate = DiscoveryGate::default();
        let (release, wait) = std::sync::mpsc::channel();
        let first = gate
            .start(move || {
                wait.recv().unwrap();
                live_audio::discovery::Discovery::default()
            })
            .unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        assert!(
            runtime
                .block_on(async { timeout(Duration::from_millis(10), first).await })
                .is_err()
        );
        drop(runtime);
        assert!(
            gate.start(live_audio::discovery::Discovery::default)
                .is_err()
        );
        release.send(()).unwrap();
        while gate.0.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        assert!(
            gate.start(live_audio::discovery::Discovery::default)
                .is_ok()
        );
    }
}
