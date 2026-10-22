use std::{os::windows::io::OwnedHandle, time::Duration};

use autodj_live_protocol::{
    AudioDiscovery, BearerToken, ClientHello, EngineCommand, EngineEvent, FrameCodec,
    HealthSnapshot, ProtocolVersion, ServerHello, WasapiCapabilities, WasapiSharedInitialization,
    WasapiSharedOutputTest,
};
use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    LaunchConfig, RuntimeRecord, SupervisorError,
    discovery::{Record, token_hex},
    process::{ManagedChild, STARTUP_TIMEOUT, wait_for_exit},
    windows,
};

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const SHARED_OUTPUT_TEST_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionIdentity {
    pub pid: u32,
    pub engine_id: Uuid,
    pub sidecar_pid: u32,
}

pub struct LiveSupervisor;

pub struct LiveSession {
    config: LaunchConfig,
    record: Record,
    token: BearerToken,
    socket: TcpStream,
    engine_handle: OwnedHandle,
    engine: Option<ManagedChild>,
    sidecar: ManagedChild,
    supports_wasapi_shared_initialization: bool,
    supports_wasapi_shared_output_test: bool,
}

impl LiveSupervisor {
    /// Launch a new pair, or reattach during an existing engine's orphan lease.
    /// An attached owner is reported as AlreadyRunning without spawning children.
    pub async fn launch(config: LaunchConfig) -> Result<LiveSession, SupervisorError> {
        let _lock = config.runtime.lock()?;
        if let Some(record) = config.runtime.read()? {
            if let Some(handle) = windows::matching_process(record.pid, record.created)? {
                return Self::attach(config, record, handle).await;
            }
            config.runtime.remove_matching(&record)?;
        }
        Self::start(config).await
    }

    /// Revalidate the protected runtime record and attach;
    /// unlike launch, a missing/stale record never starts a replacement engine.
    pub async fn reattach(runtime_record: RuntimeRecord) -> Result<LiveSession, SupervisorError> {
        let RuntimeRecord {
            config,
            record: observed,
        } = runtime_record;
        let _lock = config.runtime.lock()?;
        let record = config.runtime.read()?.ok_or(SupervisorError::StaleRecord)?;
        if record.engine_id != observed.engine_id
            || record.pid != observed.pid
            || record.created != observed.created
            || record.session_id != observed.session_id
        {
            return Err(SupervisorError::StaleRecord);
        }
        let Some(handle) = windows::matching_process(record.pid, record.created)? else {
            config.runtime.remove_matching(&record)?;
            return Err(SupervisorError::StaleRecord);
        };
        Self::attach(config, record, handle).await
    }

    async fn start(config: LaunchConfig) -> Result<LiveSession, SupervisorError> {
        config.engine.validate()?;
        config.sidecar.validate()?;
        let mut bytes = Zeroizing::new([0u8; 32]);
        getrandom::fill(&mut *bytes).map_err(|_| SupervisorError::Random)?;
        let token = BearerToken::new(*bytes);
        let session = Uuid::new_v4();
        let hex = token_hex(&token);
        let mut engine = ManagedChild::start(&config.engine, &hex, session, false).await?;
        let mut record = Record {
            version: 1,
            protection: config.runtime.protection,
            pid: engine.descriptor.pid,
            created: engine.created,
            engine_id: engine.descriptor.engine_id.unwrap(),
            port: engine.descriptor.port,
            session_id: session,
            generation: 1,
            protected_token: Record::protect(&token, config.runtime.protection)?,
            sidecar_pid: 0,
            sidecar_created: 0,
        };
        let result = async {
            let (
                mut socket,
                supports_wasapi_shared_initialization,
                supports_wasapi_shared_output_test,
            ) = connect(&record, &token).await?;
            attach(&mut socket, 1).await?;
            let mut sidecar = ManagedChild::start(&config.sidecar, &hex, session, true).await?;
            if let Err(error) = sidecar_ready(&sidecar, &token, session).await {
                sidecar.stop().await?;
                return Err(error);
            }
            record.sidecar_pid = sidecar.descriptor.pid;
            record.sidecar_created = descriptor_creation(&sidecar)?;
            if let Err(error) = config.runtime.write(&record) {
                sidecar.stop().await?;
                return Err(error);
            }
            Ok((
                socket,
                sidecar,
                supports_wasapi_shared_initialization,
                supports_wasapi_shared_output_test,
            ))
        }
        .await;
        let (
            socket,
            sidecar,
            supports_wasapi_shared_initialization,
            supports_wasapi_shared_output_test,
        ) = match result {
            Ok(pair) => pair,
            Err(error) => {
                engine.stop().await?;
                return Err(error);
            }
        };
        let engine_handle = engine
            .handle
            .try_clone()
            .map_err(|_| SupervisorError::Discovery)?;
        engine.preserve_on_drop();
        Ok(LiveSession {
            config,
            record,
            token,
            socket,
            engine_handle,
            engine: Some(engine),
            sidecar,
            supports_wasapi_shared_initialization,
            supports_wasapi_shared_output_test,
        })
    }

    async fn attach(
        config: LaunchConfig,
        mut record: Record,
        handle: OwnedHandle,
    ) -> Result<LiveSession, SupervisorError> {
        let token = record.token()?;
        let (mut socket, supports_wasapi_shared_initialization, supports_wasapi_shared_output_test) =
            connect(&record, &token).await?;
        let snapshot = health(&mut socket).await?;
        if snapshot.engine_id != record.engine_id {
            return Err(SupervisorError::Authentication);
        }
        if !snapshot.orphaned {
            return Err(SupervisorError::AlreadyRunning);
        }
        // The old job closes when the previous supervisor exits. Wait for that
        // cleanup before starting a sidecar, including a venv descendant PID.
        if let Some(old_sidecar) =
            windows::matching_process(record.sidecar_pid, record.sidecar_created)?
        {
            wait_for_exit(&old_sidecar).await?;
        }
        let generation = snapshot
            .supervisor_generation
            .max(record.generation)
            .checked_add(1)
            .ok_or(SupervisorError::Rejected)?;
        let mut sidecar =
            ManagedChild::start(&config.sidecar, &token_hex(&token), record.session_id, true)
                .await?;
        if let Err(error) = sidecar_ready(&sidecar, &token, record.session_id).await {
            sidecar.stop().await?;
            return Err(error);
        }
        record.generation = generation;
        record.sidecar_pid = sidecar.descriptor.pid;
        record.sidecar_created = descriptor_creation(&sidecar)?;
        // Write before claiming ownership: replacement failure leaves the old
        // record usable and the live engine safely inside its existing lease.
        if let Err(error) = config.runtime.write(&record) {
            sidecar.stop().await?;
            return Err(error);
        }
        if let Err(error) = attach(&mut socket, generation).await {
            sidecar.stop().await?;
            return Err(error);
        }
        Ok(LiveSession {
            config,
            record,
            token,
            socket,
            engine_handle: handle,
            engine: None,
            sidecar,
            supports_wasapi_shared_initialization,
            supports_wasapi_shared_output_test,
        })
    }
}

impl LiveSession {
    /// Replace only the engine; retain the sidecar and its session credentials on failure.
    pub async fn restart_engine(&mut self) -> Result<(), SupervisorError> {
        let _lock = self.config.runtime.lock()?;
        let _ = request(&mut self.socket, EngineCommand::Shutdown).await;
        wait_for_exit(&self.engine_handle).await?;
        let mut engine = ManagedChild::start(
            &self.config.engine,
            &token_hex(&self.token),
            self.record.session_id,
            false,
        )
        .await?;
        let mut record = self.record.clone();
        record.pid = engine.descriptor.pid;
        record.created = engine.created;
        record.engine_id = engine.descriptor.engine_id.unwrap();
        record.port = engine.descriptor.port;
        record.generation = 1;
        let result = async {
            let (
                mut socket,
                supports_wasapi_shared_initialization,
                supports_wasapi_shared_output_test,
            ) = connect(&record, &self.token).await?;
            attach(&mut socket, 1).await?;
            let handle = engine
                .handle
                .try_clone()
                .map_err(|_| SupervisorError::Discovery)?;
            self.config.runtime.write(&record)?;
            Ok((
                socket,
                handle,
                supports_wasapi_shared_initialization,
                supports_wasapi_shared_output_test,
            ))
        }
        .await;
        let (
            socket,
            engine_handle,
            supports_wasapi_shared_initialization,
            supports_wasapi_shared_output_test,
        ) = match result {
            Ok(pair) => pair,
            Err(error) => {
                engine.stop().await?;
                return Err(error);
            }
        };
        engine.preserve_on_drop();
        self.engine = Some(engine);
        self.engine_handle = engine_handle;
        self.record = record;
        self.socket = socket;
        self.supports_wasapi_shared_initialization = supports_wasapi_shared_initialization;
        self.supports_wasapi_shared_output_test = supports_wasapi_shared_output_test;
        Ok(())
    }

    /// Replace the sidecar without detaching the engine or advancing its lease.
    pub async fn restart_sidecar(&mut self) -> Result<(), SupervisorError> {
        let _lock = self.config.runtime.lock()?;
        self.sidecar.stop().await?;
        let mut sidecar = ManagedChild::start(
            &self.config.sidecar,
            &token_hex(&self.token),
            self.record.session_id,
            true,
        )
        .await?;
        let mut record = self.record.clone();
        let result = async {
            sidecar_ready(&sidecar, &self.token, self.record.session_id).await?;
            record.sidecar_pid = sidecar.descriptor.pid;
            record.sidecar_created = descriptor_creation(&sidecar)?;
            self.config.runtime.write(&record)
        }
        .await;
        if let Err(error) = result {
            sidecar.stop().await?;
            return Err(error);
        }
        self.sidecar = sidecar;
        self.record = record;
        Ok(())
    }

    pub fn identity(&self) -> SessionIdentity {
        SessionIdentity {
            pid: self.record.pid,
            engine_id: self.record.engine_id,
            sidecar_pid: self.record.sidecar_pid,
        }
    }
    pub async fn health(&mut self) -> Result<HealthSnapshot, SupervisorError> {
        if self
            .engine
            .as_ref()
            .is_some_and(|engine| !engine.healthy_output())
        {
            return Err(SupervisorError::Connection);
        }
        health(&mut self.socket).await
    }
    pub async fn audio_discovery(&mut self) -> Result<AudioDiscovery, SupervisorError> {
        match request(&mut self.socket, EngineCommand::DiscoverAudio).await? {
            EngineEvent::AudioDiscovery(discovery) => Ok(discovery),
            _ => Err(SupervisorError::Rejected),
        }
    }
    pub async fn probe_wasapi(
        &mut self,
        endpoint_id: &str,
    ) -> Result<WasapiCapabilities, SupervisorError> {
        match request(
            &mut self.socket,
            EngineCommand::ProbeWasapi {
                endpoint_id: endpoint_id.to_owned(),
            },
        )
        .await?
        {
            EngineEvent::WasapiCapabilities(capabilities) => Ok(capabilities),
            _ => Err(SupervisorError::Rejected),
        }
    }
    pub async fn initialize_wasapi_shared(
        &mut self,
        endpoint_id: &str,
    ) -> Result<WasapiSharedInitialization, SupervisorError> {
        initialize_wasapi_shared_request(
            &mut self.socket,
            self.supports_wasapi_shared_initialization,
            endpoint_id,
        )
        .await
    }
    pub async fn run_wasapi_shared_output_test(
        &mut self,
        endpoint_id: &str,
    ) -> Result<WasapiSharedOutputTest, SupervisorError> {
        run_wasapi_shared_output_test_request(
            &mut self.socket,
            self.supports_wasapi_shared_output_test,
            endpoint_id,
        )
        .await
    }
    pub async fn sidecar_healthy(&self) -> Result<bool, SupervisorError> {
        sidecar_health(&self.sidecar, &self.token, self.record.session_id)
            .await
            .map(|()| true)
    }
    pub async fn detach(mut self) -> Result<(), SupervisorError> {
        let event = request(&mut self.socket, EngineCommand::Detach).await;
        self.sidecar.stop().await?;
        match event? {
            EngineEvent::Health(snapshot) if snapshot.orphaned => Ok(()),
            _ => Err(SupervisorError::Rejected),
        }
    }
    pub async fn shutdown(mut self) -> Result<(), SupervisorError> {
        let _lock = self.config.runtime.lock()?;
        let event = request(&mut self.socket, EngineCommand::Shutdown).await;
        self.sidecar.stop().await?;
        // Even a failed response may have delivered Shutdown. Only the held
        // process handle's confirmed exit permits removing this record.
        let exited = wait_for_exit(&self.engine_handle).await;
        if exited.is_ok() {
            self.config.runtime.remove_matching(&self.record)?;
        }
        exited?;
        match event {
            Ok(EngineEvent::Stopping) => Ok(()),
            Err(_) => Ok(()),
            _ => Err(SupervisorError::Rejected),
        }
    }
}

async fn connect(
    record: &Record,
    token: &BearerToken,
) -> Result<(TcpStream, bool, bool), SupervisorError> {
    timeout(IO_TIMEOUT, async {
        let mut socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, record.port))
            .await
            .map_err(|_| SupervisorError::Connection)?;
        FrameCodec::write(
            &mut socket,
            &ClientHello {
                version: ProtocolVersion::CURRENT,
                session_id: record.session_id,
                token: token.clone(),
            },
        )
        .await
        .map_err(|_| SupervisorError::Authentication)?;
        let hello: ServerHello = FrameCodec::read_typed(&mut socket)
            .await
            .map_err(|_| SupervisorError::Authentication)?;
        if hello.version.ensure_compatible().is_err()
            || hello.engine_id != record.engine_id
            || hello.pid != record.pid
        {
            return Err(SupervisorError::Authentication);
        }
        Ok((
            socket,
            hello.supports_wasapi_shared_initialization,
            hello.supports_wasapi_shared_output_test,
        ))
    })
    .await
    .map_err(|_| SupervisorError::Connection)?
}

async fn initialize_wasapi_shared_request(
    socket: &mut TcpStream,
    supported: bool,
    endpoint_id: &str,
) -> Result<WasapiSharedInitialization, SupervisorError> {
    if !supported {
        return Err(SupervisorError::Rejected);
    }
    match request(
        socket,
        EngineCommand::InitializeWasapiShared {
            endpoint_id: endpoint_id.to_owned(),
        },
    )
    .await?
    {
        EngineEvent::WasapiSharedInitialized(initialized) => Ok(initialized),
        _ => Err(SupervisorError::Rejected),
    }
}

async fn run_wasapi_shared_output_test_request(
    socket: &mut TcpStream,
    supported: bool,
    endpoint_id: &str,
) -> Result<WasapiSharedOutputTest, SupervisorError> {
    if !supported {
        return Err(SupervisorError::Rejected);
    }
    match request_with_timeout(
        socket,
        EngineCommand::RunWasapiSharedOutputTest {
            endpoint_id: endpoint_id.to_owned(),
        },
        SHARED_OUTPUT_TEST_TIMEOUT,
    )
    .await?
    {
        EngineEvent::WasapiSharedOutputTested(tested) => Ok(tested),
        _ => Err(SupervisorError::Rejected),
    }
}

async fn request(
    socket: &mut TcpStream,
    command: EngineCommand,
) -> Result<EngineEvent, SupervisorError> {
    request_with_timeout(socket, command, IO_TIMEOUT).await
}

async fn request_with_timeout(
    socket: &mut TcpStream,
    command: EngineCommand,
    duration: Duration,
) -> Result<EngineEvent, SupervisorError> {
    timeout(duration, async {
        FrameCodec::write(socket, &command)
            .await
            .map_err(|_| SupervisorError::Connection)?;
        FrameCodec::read_typed(socket)
            .await
            .map_err(|_| SupervisorError::Connection)
    })
    .await
    .map_err(|_| SupervisorError::Connection)?
}

async fn attach(socket: &mut TcpStream, generation: u64) -> Result<(), SupervisorError> {
    match request(
        socket,
        EngineCommand::Attach {
            supervisor_generation: generation,
        },
    )
    .await?
    {
        EngineEvent::Attached => Ok(()),
        _ => Err(SupervisorError::Rejected),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unsupported_shared_initialization_is_rejected_without_consuming_the_connection() {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).await.unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        let engine_id = Uuid::new_v4();
        let server_task = tokio::spawn(async move {
            assert_eq!(
                FrameCodec::read(&mut server).await.unwrap(),
                EngineCommand::Health
            );
            FrameCodec::write(
                &mut server,
                &EngineEvent::Health(HealthSnapshot {
                    engine_id,
                    state: "attached".into(),
                    orphaned: false,
                    supervisor_generation: 1,
                    started_ns: 1,
                    lease_deadline_ns: 2,
                    extensions: Default::default(),
                }),
            )
            .await
            .unwrap();
        });

        assert_eq!(
            initialize_wasapi_shared_request(&mut client, false, "endpoint-1").await,
            Err(SupervisorError::Rejected)
        );
        assert!(matches!(
            request(&mut client, EngineCommand::Health).await.unwrap(),
            EngineEvent::Health(_)
        ));
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn unsupported_shared_output_test_is_rejected_without_consuming_the_connection() {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).await.unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        let engine_id = Uuid::new_v4();
        let server_task = tokio::spawn(async move {
            assert_eq!(
                FrameCodec::read(&mut server).await.unwrap(),
                EngineCommand::Health
            );
            FrameCodec::write(
                &mut server,
                &EngineEvent::Health(HealthSnapshot {
                    engine_id,
                    state: "attached".into(),
                    orphaned: false,
                    supervisor_generation: 1,
                    started_ns: 1,
                    lease_deadline_ns: 2,
                    extensions: Default::default(),
                }),
            )
            .await
            .unwrap();
        });

        assert_eq!(
            run_wasapi_shared_output_test_request(&mut client, false, "endpoint-1").await,
            Err(SupervisorError::Rejected)
        );
        assert!(matches!(
            request(&mut client, EngineCommand::Health).await.unwrap(),
            EngineEvent::Health(_)
        ));
        server_task.await.unwrap();
    }
}

async fn health(socket: &mut TcpStream) -> Result<HealthSnapshot, SupervisorError> {
    match request(socket, EngineCommand::Health).await? {
        EngineEvent::Health(snapshot) => Ok(snapshot),
        _ => Err(SupervisorError::Rejected),
    }
}

fn descriptor_creation(child: &ManagedChild) -> Result<u64, SupervisorError> {
    use std::os::windows::io::FromRawHandle;
    // SAFETY: query the already job-validated descriptor PID; retain a handle
    // while reading creation time to protect later PID reuse detection.
    unsafe {
        let handle = windows_sys::Win32::System::Threading::OpenProcess(
            windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            child.descriptor.pid,
        );
        if handle.is_null() {
            return Err(SupervisorError::Discovery);
        }
        windows::creation_time(&OwnedHandle::from_raw_handle(handle))
    }
}

async fn sidecar_ready(
    child: &ManagedChild,
    token: &BearerToken,
    session: Uuid,
) -> Result<(), SupervisorError> {
    timeout(STARTUP_TIMEOUT, async {
        loop {
            if !child.healthy_output() {
                return Err(SupervisorError::InvalidDescriptor);
            }
            if sidecar_health(child, token, session).await.is_ok() {
                return Ok(());
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| SupervisorError::Authentication)?
}

#[derive(Deserialize)]
struct SidecarHealth {
    protocol: ProtocolVersion,
    session_id: Uuid,
    sidecar_id: Uuid,
    status: String,
}

async fn sidecar_health(
    child: &ManagedChild,
    token: &BearerToken,
    session: Uuid,
) -> Result<(), SupervisorError> {
    if !child.healthy_output() {
        return Err(SupervisorError::Connection);
    }
    timeout(IO_TIMEOUT, async {
        let mut socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, child.descriptor.port)).await.map_err(|_| SupervisorError::Connection)?;
        let request = Zeroizing::new(format!("GET /live/v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nX-AutoDJ-Live-Session: {session}\r\nConnection: close\r\n\r\n", *token_hex(token)));
        socket.write_all(request.as_bytes()).await.map_err(|_| SupervisorError::Connection)?;
        let mut response = Zeroizing::new(Vec::new());
        socket.take(8193).read_to_end(&mut response).await.map_err(|_| SupervisorError::Connection)?;
        if response.len() > 8192 || !response.starts_with(b"HTTP/1.1 200 ") { return Err(SupervisorError::Authentication); }
        let offset = response.windows(4).position(|bytes| bytes == b"\r\n\r\n").ok_or(SupervisorError::Authentication)? + 4;
        let health: SidecarHealth = serde_json::from_slice(&response[offset..]).map_err(|_| SupervisorError::Authentication)?;
        if health.protocol.ensure_compatible().is_err() || health.session_id != session || Some(health.sidecar_id) != child.descriptor.sidecar_id || health.status != "healthy" { return Err(SupervisorError::Authentication); }
        Ok(())
    }).await.map_err(|_| SupervisorError::Connection)?
}
