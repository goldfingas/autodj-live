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
    WasapiCapabilities, WasapiSharedInitialization, WasapiSharedOutputTest,
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
const AUDIO_OPERATION_TIMEOUT: Duration = Duration::from_millis(1_500);
const SHARED_OUTPUT_TEST_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CLIENTS: usize = 64;

#[derive(Clone, Default)]
struct AudioWorkerGate(Arc<AtomicBool>);

struct AudioWorkerGuard(Arc<AtomicBool>);

impl Drop for AudioWorkerGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl AudioWorkerGate {
    fn start<T, F>(&self, operation: F) -> Result<oneshot::Receiver<T>, ()>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ())?;
        let active = self.0.clone();
        let (result, receiver) = oneshot::channel();
        let spawned = std::thread::Builder::new()
            .name("live-audio-control-request".into())
            .spawn(move || {
                let guard = AudioWorkerGuard(active);
                let result_value = operation();
                drop(guard);
                let _ = result.send(result_value);
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
    let audio_worker = AudioWorkerGate::default();
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
                clients.spawn(serve(socket, next_id, engine_id, config.clone(), state.clone(), audio_worker.clone(), shutdown.subscribe()));
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
    audio_worker: AudioWorkerGate,
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
        supports_wasapi_shared_initialization: true,
        supports_wasapi_shared_output_test: true,
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
                match command {
                    EngineCommand::DiscoverAudio => match lease.authorize_audio_operation(id) {
                        Err(event) => event,
                        Ok(()) => {
                            drop(lease);
                            let discovered = match audio_worker.start(live_audio::discovery::discover) {
                                Ok(receiver) => Some(timeout(AUDIO_OPERATION_TIMEOUT, receiver).await),
                                Err(()) => None,
                            };
                            let still_owner = state.lock().await.authorize_audio_operation(id);
                            match (discovered, still_owner) {
                                (Some(Ok(Ok(discovery))), Ok(())) => EngineEvent::AudioDiscovery(map_discovery(discovery)),
                                (None, Ok(())) => EngineEvent::Error { message: "audio operation already in progress".into() },
                                (_, Err(event)) => event,
                                _ => EngineEvent::Error { message: "audio discovery failed".into() },
                            }
                        }
                    },
                    EngineCommand::ProbeWasapi { endpoint_id } => match lease.authorize_audio_operation(id) {
                        Err(event) => event,
                        Ok(()) => {
                            drop(lease);
                            let probed = match audio_worker.start(move || live_audio::discovery::probe_wasapi(&endpoint_id)) {
                                Ok(receiver) => Some(timeout(AUDIO_OPERATION_TIMEOUT, receiver).await),
                                Err(()) => None,
                            };
                            let still_owner = state.lock().await.authorize_audio_operation(id);
                            match (probed, still_owner) {
                                (Some(Ok(Ok(Ok(capabilities)))), Ok(())) => EngineEvent::WasapiCapabilities(map_wasapi(capabilities)),
                                (None, Ok(())) => EngineEvent::Error { message: "audio operation already in progress".into() },
                                (_, Err(event)) => event,
                                _ => EngineEvent::Error { message: "WASAPI endpoint probe failed".into() },
                            }
                        }
                    },
                    EngineCommand::InitializeWasapiShared { endpoint_id } => match lease.authorize_audio_operation(id) {
                        Err(event) => event,
                        Ok(()) => {
                            drop(lease);
                            let initialized = match audio_worker.start(move || live_audio::discovery::initialize_wasapi_shared(&endpoint_id)) {
                                Ok(receiver) => Some(timeout(AUDIO_OPERATION_TIMEOUT, receiver).await),
                                Err(()) => None,
                            };
                            let still_owner = state.lock().await.authorize_audio_operation(id);
                            match (initialized, still_owner) {
                                (Some(Ok(Ok(Ok(initialized)))), Ok(())) => EngineEvent::WasapiSharedInitialized(map_wasapi_shared_initialization(initialized)),
                                (None, Ok(())) => EngineEvent::Error { message: "audio operation already in progress".into() },
                                (_, Err(event)) => event,
                                _ => EngineEvent::Error { message: "WASAPI shared initialization failed".into() },
                            }
                        }
                    },
                    EngineCommand::RunWasapiSharedOutputTest { endpoint_id } => match lease.authorize_audio_operation(id) {
                        Err(event) => event,
                        Ok(()) => {
                            drop(lease);
                            let tested = match audio_worker.start(move || live_audio::discovery::run_wasapi_shared_output_test(&endpoint_id)) {
                                Ok(receiver) => Some(timeout(SHARED_OUTPUT_TEST_TIMEOUT, receiver).await),
                                Err(()) => None,
                            };
                            let still_owner = state.lock().await.authorize_audio_operation(id);
                            match (tested, still_owner) {
                                (Some(Ok(Ok(Ok(tested)))), Ok(())) => EngineEvent::WasapiSharedOutputTested(map_wasapi_shared_output_test(tested)),
                                (None, Ok(())) => EngineEvent::Error { message: "audio operation already in progress".into() },
                                (_, Err(event)) => event,
                                _ => EngineEvent::Error { message: "WASAPI shared output test failed".into() },
                            }
                        }
                    },
                    other => lease.command(id, &replaced, other),
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

fn map_wasapi_shared_initialization(
    initialized: live_audio::discovery::WasapiSharedInitialization,
) -> WasapiSharedInitialization {
    WasapiSharedInitialization {
        endpoint_id: initialized.endpoint_id,
        requested_period_frames: initialized.requested_period_frames,
        buffer_frames: initialized.buffer_frames,
    }
}

fn map_wasapi_shared_output_test(
    tested: live_audio::discovery::WasapiSharedOutputTest,
) -> WasapiSharedOutputTest {
    WasapiSharedOutputTest {
        endpoint_id: tested.endpoint_id,
        requested_period_frames: tested.requested_period_frames,
        buffer_frames: tested.buffer_frames,
        event_notifications: tested.event_notifications,
        silent_frames_submitted: tested.silent_frames_submitted,
        elapsed_ms: tested.elapsed_ms,
    }
}

fn map_wasapi(capabilities: live_audio::discovery::WasapiCapabilities) -> WasapiCapabilities {
    WasapiCapabilities {
        endpoint_id: capabilities.endpoint_id,
        channels: capabilities.channels,
        sample_rate_hz: capabilities.sample_rate_hz,
        bits_per_sample: capabilities.bits_per_sample,
        default_period_100ns: capabilities.default_period_100ns,
        minimum_period_100ns: capabilities.minimum_period_100ns,
        exclusive_shared_mix_supported: capabilities.exclusive_shared_mix_supported,
        exclusive_shared_format_sample_rates_hz: Some(
            capabilities.exclusive_shared_format_sample_rates_hz,
        ),
        shared_mode_engine_periods: Some(autodj_live_protocol::SharedModeEnginePeriods {
            default_frames: capabilities.shared_mode_engine_periods.default_frames,
            fundamental_frames: capabilities.shared_mode_engine_periods.fundamental_frames,
            minimum_frames: capabilities.shared_mode_engine_periods.minimum_frames,
            maximum_frames: capabilities.shared_mode_engine_periods.maximum_frames,
        }),
    }
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
        let gate = AudioWorkerGate::default();
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
