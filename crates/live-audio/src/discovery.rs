//! Read-only inventory, not audio capability negotiation.

mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointState {
    Active,
    Disabled,
    NotPresent,
    Unplugged,
    Unknown(u32),
}

#[derive(Debug)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
    pub direction: Direction,
    pub state: EndpointState,
}

/// A 64-bit registry entry, not proof that the hardware or driver works.
#[derive(Debug)]
pub struct AsioRegistration {
    pub registration: String,
    pub clsid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasapiCapabilities {
    pub endpoint_id: String,
    pub channels: u16,
    pub sample_rate_hz: u32,
    pub bits_per_sample: u16,
    pub default_period_100ns: u64,
    pub minimum_period_100ns: u64,
    pub exclusive_shared_mix_supported: bool,
}

#[derive(Debug, Default)]
pub struct Discovery {
    pub endpoints: Vec<Endpoint>,
    pub asio_drivers: Vec<AsioRegistration>,
    pub issues: Vec<String>,
}

impl Discovery {
    fn record_endpoint(
        &mut self,
        entry: Result<Endpoint, String>,
        direction: Direction,
        index: u32,
    ) {
        match entry {
            Ok(entry) => self.endpoints.push(entry),
            Err(error) => self
                .issues
                .push(format!("{direction:?} endpoint {index}: {error}")),
        }
    }
}

fn endpoint_label(id: &str, name: Result<String, String>, issues: &mut Vec<String>) -> String {
    match name {
        Ok(name) if !name.is_empty() => name,
        other => {
            issues.push(format!("Endpoint {id} name unavailable: {other:?}"));
            id.to_owned()
        }
    }
}

/// Blocking, control-thread inventory. No streams or drivers are initialized.
pub fn discover() -> Discovery {
    match std::thread::Builder::new()
        .name("live-audio-discovery".into())
        .spawn(windows::discover)
    {
        Ok(worker) => worker.join().unwrap_or_else(|_| Discovery {
            issues: vec!["Audio discovery worker panicked".into()],
            ..Discovery::default()
        }),
        Err(error) => Discovery {
            issues: vec![format!("Could not start audio discovery: {error}")],
            ..Discovery::default()
        },
    }
}

/// Blocking, control-thread probe of the WASAPI shared engine. No stream is initialized.
pub fn probe_wasapi(endpoint_id: &str) -> Result<WasapiCapabilities, &'static str> {
    let endpoint_id = endpoint_id.to_owned();
    std::thread::Builder::new()
        .name("live-audio-wasapi-probe".into())
        .spawn(move || windows::probe_wasapi(&endpoint_id))
        .map_err(|_| "could not start WASAPI probe")?
        .join()
        .map_err(|_| "WASAPI probe worker panicked")?
}

fn wasapi_capabilities(
    endpoint_id: String,
    channels: u16,
    sample_rate_hz: u32,
    bits_per_sample: u16,
    default_period_100ns: i64,
    minimum_period_100ns: i64,
    exclusive_shared_mix_supported: bool,
) -> Result<WasapiCapabilities, &'static str> {
    if endpoint_id.is_empty()
        || channels == 0
        || sample_rate_hz == 0
        || bits_per_sample == 0
        || default_period_100ns < 0
        || minimum_period_100ns < 0
        || minimum_period_100ns > default_period_100ns
    {
        return Err("WASAPI returned invalid capabilities");
    }
    Ok(WasapiCapabilities {
        endpoint_id,
        channels,
        sample_rate_hz,
        bits_per_sample,
        default_period_100ns: default_period_100ns as u64,
        minimum_period_100ns: minimum_period_100ns as u64,
        exclusive_shared_mix_supported,
    })
}

fn endpoint_state(state: u32) -> EndpointState {
    match state {
        1 => EndpointState::Active,
        2 => EndpointState::Disabled,
        4 => EndpointState::NotPresent,
        8 => EndpointState::Unplugged,
        other => EndpointState::Unknown(other),
    }
}

fn registry_text(text: &[u16]) -> Result<String, &'static str> {
    let Some((&0, value)) = text.split_last() else {
        return Err("Registry text is not NUL-terminated");
    };
    if value.is_empty() || value.contains(&0) {
        return Err("Registry text is empty or contains an embedded NUL");
    }
    String::from_utf16(value).map_err(|_| "Registry text is not valid UTF-16")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_states_do_not_confuse_unavailable_with_active() {
        assert_eq!(endpoint_state(1), EndpointState::Active);
        assert_eq!(endpoint_state(2), EndpointState::Disabled);
        assert_eq!(endpoint_state(4), EndpointState::NotPresent);
        assert_eq!(endpoint_state(8), EndpointState::Unplugged);
        assert_eq!(endpoint_state(0), EndpointState::Unknown(0));
        assert_eq!(endpoint_state(3), EndpointState::Unknown(3));
    }

    #[test]
    fn returned_registry_buffer_requires_one_terminator_and_valid_unicode() {
        assert_eq!(registry_text(&[65, 0]).unwrap(), "A");
        for malformed in [&[][..], &[0], &[65], &[65, 0, 66, 0], &[0xD800, 0]] {
            assert!(registry_text(malformed).is_err(), "{malformed:?}");
        }
        let clsid = "{01234567-89AB-CDEF-0123-456789ABCDEF}";
        let text: Vec<u16> = clsid.encode_utf16().chain(Some(0)).collect();
        assert_eq!(registry_text(&text).unwrap(), clsid);
    }

    #[test]
    fn partial_failure_preserves_endpoints_with_duplicate_names() {
        let mut result = Discovery::default();
        for id in ["first", "second"] {
            result.record_endpoint(
                Ok(Endpoint {
                    id: id.into(),
                    name: "Speakers".into(),
                    direction: Direction::Output,
                    state: EndpointState::Active,
                }),
                Direction::Output,
                0,
            );
        }
        result.record_endpoint(Err("Device removed".into()), Direction::Input, 1);
        assert_eq!(result.endpoints.len(), 2);
        assert_eq!(result.endpoints[0].id, "first");
        assert_eq!(result.endpoints[1].id, "second");
        assert_eq!(result.issues.len(), 1);
        assert!(result.issues[0].contains("Device removed"));
    }

    #[test]
    fn missing_name_falls_back_to_id_without_hiding_error() {
        let mut issues = vec![];
        assert_eq!(
            endpoint_label("endpoint-id", Err("Not present".into()), &mut issues),
            "endpoint-id"
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(
            endpoint_label("other", Ok(String::new()), &mut issues),
            "other"
        );
        assert_eq!(issues.len(), 2);
        assert_eq!(
            endpoint_label("third", Ok("Speakers".into()), &mut issues),
            "Speakers"
        );
        assert_eq!(issues.len(), 2);
    }

    #[test]
    fn wasapi_capabilities_require_a_real_format_and_nonnegative_periods() {
        assert_eq!(
            wasapi_capabilities("endpoint-1".into(), 8, 48_000, 32, 100_000, 30_000, true).unwrap(),
            WasapiCapabilities {
                endpoint_id: "endpoint-1".into(),
                channels: 8,
                sample_rate_hz: 48_000,
                bits_per_sample: 32,
                default_period_100ns: 100_000,
                minimum_period_100ns: 30_000,
                exclusive_shared_mix_supported: true,
            }
        );

        for malformed in [
            wasapi_capabilities("id".into(), 0, 48_000, 32, 1, 1, false),
            wasapi_capabilities("id".into(), 2, 0, 32, 1, 1, false),
            wasapi_capabilities("id".into(), 2, 48_000, 0, 1, 1, false),
            wasapi_capabilities("id".into(), 2, 48_000, 32, -1, 1, false),
            wasapi_capabilities("id".into(), 2, 48_000, 32, 1, -1, false),
            wasapi_capabilities("id".into(), 2, 48_000, 32, 1, 2, false),
        ] {
            assert!(malformed.is_err());
        }
    }
}
