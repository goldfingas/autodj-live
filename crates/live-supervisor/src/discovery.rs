use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};

use autodj_live_protocol::BearerToken;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{LaunchConfig, SupervisorError, windows};

/// Opaque, privacy-validated discovery handle. Executable paths are retained
/// from trusted caller configuration; no executable is read from the record.
pub struct RuntimeRecord {
    pub(crate) config: LaunchConfig,
    pub(crate) record: Record,
}

impl RuntimeRecord {
    pub fn load(config: LaunchConfig) -> Result<Self, SupervisorError> {
        let _lock = config.runtime.lock()?;
        let record = config.runtime.read()?.ok_or(SupervisorError::StaleRecord)?;
        record.token()?;
        Ok(Self { config, record })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Protection {
    Installed,
    Portable,
}

/// A dedicated per-user runtime directory, not the catalog or session export.
/// Its parent must already exist. Existing insecure directories are refused;
/// their ACL is never silently rewritten around an already-exposed record.
#[derive(Clone)]
pub struct RuntimeStore {
    directory: PathBuf,
    pub protection: Protection,
}

impl RuntimeStore {
    pub fn new(directory: PathBuf, protection: Protection) -> Self {
        Self {
            directory,
            protection,
        }
    }
    pub fn installed() -> Result<Self, SupervisorError> {
        let local = std::env::var_os("LOCALAPPDATA").ok_or(SupervisorError::Privacy)?;
        Ok(Self::new(
            PathBuf::from(local).join("AutoDJ-Live-runtime"),
            Protection::Installed,
        ))
    }
    pub fn record_path(&self) -> PathBuf {
        self.directory.join("runtime.json")
    }

    pub(crate) fn lock(&self) -> Result<File, SupervisorError> {
        if !self.directory.is_absolute() {
            return Err(SupervisorError::Privacy);
        }
        windows::private_directory(&self.directory)?;
        let lock_path = self.directory.join("supervisor.lock");
        if lock_path
            .try_exists()
            .map_err(|_| SupervisorError::Discovery)?
        {
            windows::private_file(&lock_path, false)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| SupervisorError::Discovery)?;
        // Validate newly inherited ACLs before using the lock as the authority
        // for discovery operations.
        windows::private_file(&lock_path, false)?;
        windows::lock(&file)?;
        Ok(file)
    }

    pub(crate) fn read(&self) -> Result<Option<Record>, SupervisorError> {
        let path = self.record_path();
        if !path.try_exists().map_err(|_| SupervisorError::Discovery)? {
            return Ok(None);
        }
        windows::private_file(&path, false)?;
        let mut bytes = Zeroizing::new(Vec::new());
        File::open(path)
            .map_err(|_| SupervisorError::Discovery)?
            .take(16385)
            .read_to_end(&mut bytes)
            .map_err(|_| SupervisorError::Discovery)?;
        if bytes.len() > 16384 {
            return Err(SupervisorError::Discovery);
        }
        let record: Record =
            serde_json::from_slice(&bytes).map_err(|_| SupervisorError::Discovery)?;
        if record.protection != self.protection
            || record.version != 1
            || record.pid == 0
            || record.port == 0
            || record.engine_id.is_nil()
            || record.session_id.is_nil()
            || record.generation == 0
            || record.created == 0
        {
            return Err(SupervisorError::Discovery);
        }
        Ok(Some(record))
    }

    pub(crate) fn write(&self, record: &Record) -> Result<(), SupervisorError> {
        let temporary = self
            .directory
            .join(format!(".runtime-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| SupervisorError::Discovery)?;
            // Portable records contain the raw bearer token. Never write even
            // protected bytes until the newly inherited file ACL is verified.
            windows::private_file(&temporary, false)?;
            let bytes =
                Zeroizing::new(serde_json::to_vec(record).map_err(|_| SupervisorError::Discovery)?);
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| SupervisorError::Discovery)?;
            drop(file);
            windows::replace(&temporary, &self.record_path())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    /// Caller must hold the discovery lock and have confirmed this engine's
    /// exit or proved its PID/creation-time identity stale.
    pub(crate) fn remove_matching(&self, expected: &Record) -> Result<(), SupervisorError> {
        if self.read()?.is_some_and(|record| {
            record.engine_id == expected.engine_id
                && record.pid == expected.pid
                && record.created == expected.created
        }) {
            std::fs::remove_file(self.record_path()).map_err(|_| SupervisorError::Discovery)?;
        }
        Ok(())
    }
}

// Intentionally no Debug: session identity and protected credentials are not
// UI/log payloads. DPAPI binds installed records to the current Windows user.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub version: u32,
    pub protection: Protection,
    pub pid: u32,
    pub created: u64,
    pub engine_id: Uuid,
    pub port: u16,
    pub session_id: Uuid,
    pub generation: u64,
    pub protected_token: Vec<u8>,
    pub sidecar_pid: u32,
    pub sidecar_created: u64,
}

impl Record {
    pub fn protect(
        token: &BearerToken,
        protection: Protection,
    ) -> Result<Vec<u8>, SupervisorError> {
        match protection {
            Protection::Installed => windows::protect(token.as_bytes(), false),
            Protection::Portable => Ok(token.as_bytes().to_vec()),
        }
    }
    pub fn token(&self) -> Result<BearerToken, SupervisorError> {
        let bytes = Zeroizing::new(match self.protection {
            Protection::Installed => windows::protect(&self.protected_token, true)?,
            Protection::Portable => self.protected_token.clone(),
        });
        Ok(BearerToken::new(
            bytes
                .as_slice()
                .try_into()
                .map_err(|_| SupervisorError::Discovery)?,
        ))
    }
}

pub(crate) fn token_hex(token: &BearerToken) -> Zeroizing<String> {
    use std::fmt::Write;
    let mut text = Zeroizing::new(String::with_capacity(64));
    for byte in token.as_bytes() {
        let _ = write!(text, "{byte:02x}");
    }
    text
}
