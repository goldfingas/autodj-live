use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::File,
    io::Read,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::PathBuf,
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use autodj_live_protocol::ProtocolVersion;
use serde::Deserialize;
use tokio::{
    sync::oneshot,
    time::{sleep, timeout},
};
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation},
    Security::SECURITY_ATTRIBUTES,
    System::{
        JobObjects::{
            CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject,
        },
        Pipes::CreatePipe,
        Threading::{
            CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
            DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
            InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
            TerminateProcess, UpdateProcThreadAttribute,
        },
    },
};
use zeroize::Zeroizing;

use crate::{SupervisorError, windows};

pub(crate) const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);
const DESCRIPTOR_LIMIT: usize = 8192;

#[derive(Clone)]
pub struct ChildCommand {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
}

impl ChildCommand {
    pub fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            arguments: Vec::new(),
        }
    }
    pub fn args(mut self, arguments: impl IntoIterator<Item = OsString>) -> Self {
        self.arguments.extend(arguments);
        self
    }
    pub fn bundled_python(executable: PathBuf) -> Self {
        Self::new(executable).args(
            [
                "-m",
                "autodj",
                "_live-sidecar",
                "--host",
                "127.0.0.1",
                "--port",
                "0",
            ]
            .map(OsString::from),
        )
    }
    pub(crate) fn validate(&self) -> Result<(), SupervisorError> {
        if !self.executable.is_absolute()
            || !self.executable.is_file()
            || !self
                .executable
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        {
            return Err(SupervisorError::InvalidExecutable);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Descriptor {
    pub protocol: ProtocolVersion,
    pub port: u16,
    pub pid: u32,
    pub engine_id: Option<Uuid>,
    pub sidecar_id: Option<Uuid>,
}

pub(crate) struct ManagedChild {
    pub handle: OwnedHandle,
    pub created: u64,
    pub descriptor: Descriptor,
    job: Option<OwnedHandle>,
    kill_on_drop: bool,
    noisy: Arc<AtomicBool>,
}

impl ManagedChild {
    pub async fn start(
        command: &ChildCommand,
        token: &str,
        session: Uuid,
        sidecar: bool,
    ) -> Result<Self, SupervisorError> {
        command.validate()?;
        let (handle, job, stdout, stderr) = spawn(command, token, session, sidecar)?;
        let noisy = Arc::new(AtomicBool::new(false));
        let mut child = Self {
            handle,
            created: 0,
            descriptor: Descriptor {
                protocol: ProtocolVersion::CURRENT,
                port: 0,
                pid: 0,
                engine_id: None,
                sidecar_id: None,
            },
            job,
            kill_on_drop: true,
            noisy: noisy.clone(),
        };
        child.created = windows::creation_time(&child.handle)?;
        // Discard diagnostics in a fixed-size buffer. Untrusted stderr, including
        // credentials printed by a failed child, is never retained or surfaced.
        std::thread::spawn(move || {
            let mut stderr = stderr;
            let mut buffer = [0u8; 4096];
            while matches!(stderr.read(&mut buffer), Ok(count) if count > 0) {}
        });
        let monitor_handle = child
            .handle
            .try_clone()
            .map_err(|_| SupervisorError::Spawn)?;
        let (sender, receiver) = oneshot::channel();
        std::thread::spawn(move || {
            let mut stdout = stdout;
            let result = read_descriptor(&mut stdout);
            let valid = result.is_ok();
            let _ = sender.send(result);
            if valid {
                let mut byte = [0];
                if matches!(stdout.read(&mut byte), Ok(count) if count > 0) {
                    noisy.store(true, Ordering::Release);
                    // SAFETY: this owned handle belongs to the launched child.
                    unsafe {
                        TerminateProcess(monitor_handle.as_raw_handle(), 1);
                    }
                }
            }
        });
        let result = match timeout(STARTUP_TIMEOUT, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(SupervisorError::InvalidDescriptor),
            Err(_) => Err(SupervisorError::StartupTimeout),
        };
        let descriptor = match result {
            Ok(descriptor) => descriptor,
            Err(error) => {
                child.stop().await?;
                return Err(error);
            }
        };
        if !child.valid_descriptor(&descriptor, sidecar) {
            child.stop().await?;
            return Err(SupervisorError::InvalidDescriptor);
        }
        child.descriptor = descriptor;
        Ok(child)
    }

    fn valid_descriptor(&self, descriptor: &Descriptor, sidecar: bool) -> bool {
        if descriptor.protocol.ensure_compatible().is_err()
            || descriptor.port == 0
            || descriptor.pid == 0
            || self.noisy.load(Ordering::Acquire)
        {
            return false;
        }
        if sidecar {
            if descriptor.engine_id.is_some() || descriptor.sidecar_id.is_none_or(|id| id.is_nil())
            {
                return false;
            }
            // The bundled Python executable can be a venv redirector. Require
            // the descriptor process to belong to our job, not to match its PID.
            // SAFETY: all pointers refer to live owned handles/output memory.
            unsafe {
                let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, descriptor.pid);
                if process.is_null() {
                    return false;
                }
                let process = OwnedHandle::from_raw_handle(process);
                let mut inside = 0;
                IsProcessInJob(
                    process.as_raw_handle(),
                    self.job.as_ref().unwrap().as_raw_handle(),
                    &mut inside,
                ) != 0
                    && inside != 0
            }
        } else {
            // SAFETY: the query uses our own process handle.
            let pid = unsafe {
                windows_sys::Win32::System::Threading::GetProcessId(self.handle.as_raw_handle())
            };
            descriptor.pid == pid
                && descriptor.sidecar_id.is_none()
                && descriptor.engine_id.is_some_and(|id| !id.is_nil())
        }
    }

    pub fn preserve_on_drop(&mut self) {
        self.kill_on_drop = false;
    }
    pub fn healthy_output(&self) -> bool {
        !self.noisy.load(Ordering::Acquire) && !windows::exited(&self.handle)
    }

    pub async fn stop(&mut self) -> Result<(), SupervisorError> {
        self.terminate();
        wait_for_exit(&self.handle).await
    }

    fn terminate(&self) {
        // SAFETY: owned handles were created with termination rights. Killing
        // the job includes Python redirectors and all sidecar descendants.
        unsafe {
            if let Some(job) = &self.job {
                TerminateJobObject(job.as_raw_handle(), 1);
            } else {
                TerminateProcess(self.handle.as_raw_handle(), 1);
            }
        }
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        if self.kill_on_drop {
            self.terminate();
        }
    }
}

pub(crate) async fn wait_for_exit(handle: &OwnedHandle) -> Result<(), SupervisorError> {
    timeout(STARTUP_TIMEOUT, async {
        while !windows::exited(handle) {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| SupervisorError::ExitUnconfirmed)
}

fn read_descriptor(source: &mut File) -> Result<Descriptor, SupervisorError> {
    let mut line = Vec::with_capacity(256);
    loop {
        let mut byte = [0];
        if source.read_exact(&mut byte).is_err() {
            return Err(SupervisorError::InvalidDescriptor);
        }
        line.push(byte[0]);
        if line.len() > DESCRIPTOR_LIMIT {
            return Err(SupervisorError::DescriptorTooLarge);
        }
        if byte[0] == b'\n' {
            break;
        }
    }
    let value: serde_json::Value =
        serde_json::from_slice(&line).map_err(|_| SupervisorError::InvalidDescriptor)?;
    if !value.as_object().is_some_and(|fields| fields.len() == 4) {
        return Err(SupervisorError::InvalidDescriptor);
    }
    // Decode from the original bytes as well: serde's struct decoder rejects
    // duplicate keys which the generic object representation would collapse.
    serde_json::from_slice(&line).map_err(|_| SupervisorError::InvalidDescriptor)
}

// Windows CreateProcess accepts a command-line string. Encode each supplied
// argument with the documented CRT quoting rules; no shell interprets it.
fn quote(value: &OsStr, output: &mut Vec<u16>) -> Result<(), SupervisorError> {
    output.push(b'"' as u16);
    let mut slashes = 0;
    for character in value.encode_wide() {
        if character == 0 {
            return Err(SupervisorError::Spawn);
        }
        if character == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        output.extend(std::iter::repeat_n(
            b'\\' as u16,
            if character == b'"' as u16 {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        output.push(character);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    Ok(())
}

fn pipe() -> Result<(File, OwnedHandle), SupervisorError> {
    let mut read = null_mut();
    let mut write = null_mut();
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    // SAFETY: valid output pointers; ownership is immediately transferred to
    // RAII handles. Only the write end is allowed to inherit into the child.
    unsafe {
        if CreatePipe(&mut read, &mut write, &attributes, 0) == 0 {
            return Err(SupervisorError::Spawn);
        }
        let read = File::from_raw_handle(read);
        let write = OwnedHandle::from_raw_handle(write);
        if SetHandleInformation(read.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(SupervisorError::Spawn);
        }
        Ok((read, write))
    }
}

struct Attributes {
    buffer: Vec<usize>,
}
impl Attributes {
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.buffer.as_mut_ptr().cast()
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: an instance is constructed only after successful initialization.
        unsafe {
            DeleteProcThreadAttributeList(self.pointer());
        }
    }
}

fn spawn(
    command: &ChildCommand,
    token: &str,
    session: Uuid,
    sidecar: bool,
) -> Result<(OwnedHandle, Option<OwnedHandle>, File, File), SupervisorError> {
    let application = windows::wide(command.executable.as_os_str())?;
    let mut arguments = Vec::new();
    quote(command.executable.as_os_str(), &mut arguments)?;
    for argument in &command.arguments {
        arguments.push(b' ' as u16);
        quote(argument, &mut arguments)?;
    }
    arguments.push(0);
    let mut environment: BTreeMap<OsString, OsString> = std::env::vars_os()
        .filter(|(key, _)| {
            !key.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("AUTODJ_LIVE_")
        })
        .collect();
    environment.insert("AUTODJ_RUNTIME".into(), "current".into());
    environment.insert("AUTODJ_LIVE_TOKEN".into(), token.into());
    environment.insert("AUTODJ_LIVE_SESSION_ID".into(), session.to_string().into());
    let mut environment_block = Zeroizing::new(Vec::new());
    for (key, value) in environment {
        environment_block.extend(key.encode_wide());
        environment_block.push(b'=' as u16);
        environment_block.extend(value.encode_wide());
        environment_block.push(0);
    }
    environment_block.push(0);
    let (stdout, stdout_write) = pipe()?;
    let (stderr, stderr_write) = pipe()?;
    let stdin = File::open("NUL").map_err(|_| SupervisorError::Spawn)?;
    let mut job = None;
    // SAFETY: the attribute list stores borrowed pointers whose backing data
    // lives through CreateProcessW. The explicit handle list excludes the job
    // handle, so kill-on-close remains tied to the supervisor alone. JOB_LIST
    // assigns ownership atomically before the child can execute or spawn.
    unsafe {
        if SetHandleInformation(
            stdin.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        ) == 0
        {
            return Err(SupervisorError::Spawn);
        }
        if sidecar {
            let handle = CreateJobObjectW(null(), null());
            if handle.is_null() {
                return Err(SupervisorError::Spawn);
            }
            let handle = OwnedHandle::from_raw_handle(handle);
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                handle.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
            ) == 0
            {
                return Err(SupervisorError::Spawn);
            }
            job = Some(handle);
        }
        let mut size = 0;
        let count = if sidecar { 2 } else { 1 };
        InitializeProcThreadAttributeList(null_mut(), count, 0, &mut size);
        let mut buffer = vec![0usize; size.div_ceil(size_of::<usize>())];
        if InitializeProcThreadAttributeList(buffer.as_mut_ptr().cast(), count, 0, &mut size) == 0 {
            return Err(SupervisorError::Spawn);
        }
        let mut attributes = Attributes { buffer };
        let handles = [
            stdin.as_raw_handle(),
            stdout_write.as_raw_handle(),
            stderr_write.as_raw_handle(),
        ];
        if UpdateProcThreadAttribute(
            attributes.pointer(),
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr().cast(),
            size_of_val(&handles),
            null_mut(),
            null(),
        ) == 0
        {
            return Err(SupervisorError::Spawn);
        }
        let job_handle = job.as_ref().map(AsRawHandle::as_raw_handle);
        if let Some(job_handle) = &job_handle
            && UpdateProcThreadAttribute(
                attributes.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                (job_handle as *const HANDLE).cast(),
                size_of::<HANDLE>(),
                null_mut(),
                null(),
            ) == 0
        {
            return Err(SupervisorError::Spawn);
        }
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = stdin.as_raw_handle();
        startup.StartupInfo.hStdOutput = stdout_write.as_raw_handle();
        startup.StartupInfo.hStdError = stderr_write.as_raw_handle();
        startup.lpAttributeList = attributes.pointer();
        let mut process = PROCESS_INFORMATION::default();
        if CreateProcessW(
            application.as_ptr(),
            arguments.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
            environment_block.as_ptr().cast(),
            null(),
            &startup.StartupInfo,
            &mut process,
        ) == 0
        {
            return Err(SupervisorError::Spawn);
        }
        let process_handle = OwnedHandle::from_raw_handle(process.hProcess);
        let _thread_handle = OwnedHandle::from_raw_handle(process.hThread);
        Ok((process_handle, job, stdout, stderr))
    }
}
