use std::{env, io::Write, time::Duration};

use autodj_live_protocol::{BearerToken, ProtocolVersion};
use serde::Serialize;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

pub struct Config {
    pub token: BearerToken,
    pub session_id: Uuid,
    pub reconnect: Duration,
}

impl Config {
    /// Called once from main, before any threads exist.
    pub fn from_env() -> Result<Self, &'static str> {
        let source = env::var("AUTODJ_LIVE_TOKEN");
        // SAFETY: main calls this before building the runtime or spawning threads.
        unsafe { env::remove_var("AUTODJ_LIVE_TOKEN") };
        let mut source = Zeroizing::new(
            source.map_err(|_| "AUTODJ_LIVE_TOKEN must be 64 lowercase hex characters")?,
        );
        if source.len() != 64
            || !source
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("AUTODJ_LIVE_TOKEN must be 64 lowercase hex characters");
        }
        let mut bytes = Zeroizing::new([0_u8; 32]);
        for (index, pair) in source.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let nibble = |byte: u8| {
                if byte <= b'9' {
                    byte - b'0'
                } else {
                    byte - b'a' + 10
                }
            };
            bytes[index] = (nibble(pair[0]) << 4) | nibble(pair[1]);
        }
        let token = BearerToken::new(*bytes);
        source.zeroize();
        bytes.zeroize();
        let session_id = env::var("AUTODJ_LIVE_SESSION_ID")
            .ok()
            .and_then(|value| Uuid::parse_str(&value).ok())
            .ok_or("AUTODJ_LIVE_SESSION_ID must be a UUID")?;
        let reconnect = match env::var("AUTODJ_LIVE_RECONNECT_MS") {
            Err(env::VarError::NotPresent) => Duration::from_secs(30),
            Ok(value) if cfg!(debug_assertions) => {
                let milliseconds = value
                    .parse::<u64>()
                    .ok()
                    .filter(|ms| (1..=30_000).contains(ms))
                    .ok_or("AUTODJ_LIVE_RECONNECT_MS must be 1..=30000 for tests")?;
                Duration::from_millis(milliseconds)
            }
            _ => return Err("AUTODJ_LIVE_RECONNECT_MS is available only in test/debug builds"),
        };
        Ok(Self {
            token,
            session_id,
            reconnect,
        })
    }
}

#[derive(Serialize)]
struct Descriptor {
    protocol: ProtocolVersion,
    port: u16,
    pid: u32,
    engine_id: Uuid,
}

pub fn emit_descriptor(port: u16, engine_id: Uuid) -> Result<(), &'static str> {
    let descriptor = Descriptor {
        protocol: ProtocolVersion::CURRENT,
        port,
        pid: std::process::id(),
        engine_id,
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &descriptor)
        .map_err(|_| "could not write startup descriptor")?;
    writeln!(stdout)
        .and_then(|()| stdout.flush())
        .map_err(|_| "could not flush startup descriptor")
}
