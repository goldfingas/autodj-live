//! Shared wire types for AutoDJ Live process control.

mod codec;

use std::{collections::BTreeMap, fmt};

pub use codec::{FrameCodec, ProtocolError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    pub const CURRENT: Self = Self { major: 1, minor: 0 };

    pub fn ensure_compatible(self) -> Result<(), ProtocolError> {
        if self.major != Self::CURRENT.major {
            return Err(ProtocolError::IncompatibleMajor {
                expected: Self::CURRENT.major,
                received: self.major,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct BearerToken([u8; 32]);

impl BearerToken {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for BearerToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BearerToken([REDACTED])")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientHello {
    pub version: ProtocolVersion,
    pub session_id: Uuid,
    pub token: BearerToken,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServerHello {
    pub version: ProtocolVersion,
    pub engine_id: Uuid,
    pub pid: u32,
    pub lease_deadline_ns: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HealthSnapshot {
    pub engine_id: Uuid,
    pub state: String,
    pub orphaned: bool,
    pub supervisor_generation: u64,
    pub started_ns: u64,
    pub lease_deadline_ns: u64,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, rmpv::Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioDirection {
    Input,
    Output,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioEndpointState {
    Active,
    Disabled,
    NotPresent,
    Unplugged,
    Unknown(u32),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AudioEndpoint {
    pub id: String,
    pub name: String,
    pub direction: AudioDirection,
    pub state: AudioEndpointState,
}

/// A registry entry only; this does not assert driver or hardware usability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AsioRegistration {
    pub registration: String,
    pub clsid: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiscovery {
    pub endpoints: Vec<AudioEndpoint>,
    pub asio_drivers: Vec<AsioRegistration>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WasapiCapabilities {
    pub endpoint_id: String,
    pub channels: u16,
    pub sample_rate_hz: u32,
    pub bits_per_sample: u16,
    pub default_period_100ns: u64,
    pub minimum_period_100ns: u64,
    pub exclusive_shared_mix_supported: bool,
    #[serde(default)]
    pub exclusive_shared_format_sample_rates_hz: Option<Vec<u32>>,
    #[serde(default)]
    pub shared_mode_engine_periods: Option<SharedModeEnginePeriods>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedModeEnginePeriods {
    pub default_frames: u32,
    pub fundamental_frames: u32,
    pub minimum_frames: u32,
    pub maximum_frames: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WasapiSharedInitialization {
    pub endpoint_id: String,
    pub requested_period_frames: u32,
    pub buffer_frames: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EngineCommand {
    Health,
    DiscoverAudio,
    ProbeWasapi { endpoint_id: String },
    InitializeWasapiShared { endpoint_id: String },
    Attach { supervisor_generation: u64 },
    Detach,
    Shutdown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum EngineEvent {
    Attached,
    Health(HealthSnapshot),
    AudioDiscovery(AudioDiscovery),
    WasapiCapabilities(WasapiCapabilities),
    WasapiSharedInitialized(WasapiSharedInitialization),
    Stopping,
    Error { message: String },
}
