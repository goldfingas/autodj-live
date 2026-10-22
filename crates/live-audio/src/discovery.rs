//! Control-thread Windows audio inventory, probing, and transient buffer checks.

mod windows;

const OFFICIAL_SAMPLE_RATES_HZ: [u32; 3] = [44_100, 48_000, 96_000];

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
    pub exclusive_shared_format_sample_rates_hz: Vec<u32>,
    pub shared_mode_engine_periods: SharedModeEnginePeriods,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedModeEnginePeriods {
    pub default_frames: u32,
    pub fundamental_frames: u32,
    pub minimum_frames: u32,
    pub maximum_frames: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasapiSharedInitialization {
    pub endpoint_id: String,
    pub requested_period_frames: u32,
    pub buffer_frames: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasapiSharedOutputTest {
    pub endpoint_id: String,
    pub requested_period_frames: u32,
    pub buffer_frames: u32,
    pub event_notifications: u32,
    pub silent_frames_submitted: u64,
    pub elapsed_ms: u64,
}

impl SharedModeEnginePeriods {
    pub fn supports_period_frames(&self, frames: u32) -> bool {
        frames >= self.minimum_frames
            && frames <= self.maximum_frames
            && frames.is_multiple_of(self.fundamental_frames)
    }
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

/// Blocking, control-thread shared-buffer check. The stream is never started
/// and the initialized client is released before this function returns.
pub fn initialize_wasapi_shared(
    endpoint_id: &str,
) -> Result<WasapiSharedInitialization, &'static str> {
    let endpoint_id = endpoint_id.to_owned();
    std::thread::Builder::new()
        .name("live-audio-wasapi-shared-init".into())
        .spawn(move || windows::initialize_wasapi_shared(&endpoint_id))
        .map_err(|_| "could not start WASAPI shared initialization")?
        .join()
        .map_err(|_| "WASAPI shared initialization worker panicked")?
}

/// Blocking, control-thread shared render exercise. It submits silence for
/// one second, then stops and releases the stream before returning.
pub fn run_wasapi_shared_output_test(
    endpoint_id: &str,
) -> Result<WasapiSharedOutputTest, &'static str> {
    let endpoint_id = endpoint_id.to_owned();
    std::thread::Builder::new()
        .name("live-audio-wasapi-silent-output".into())
        .spawn(move || windows::run_wasapi_shared_output_test(&endpoint_id))
        .map_err(|_| "could not start WASAPI shared output test")?
        .join()
        .map_err(|_| "WASAPI shared output test worker panicked")?
}

fn select_shared_period(periods: &SharedModeEnginePeriods) -> u32 {
    const TARGET_FRAMES: u32 = 128;
    if periods.supports_period_frames(TARGET_FRAMES) {
        TARGET_FRAMES
    } else {
        periods.default_frames
    }
}

fn wasapi_shared_initialization(
    endpoint_id: String,
    requested_period_frames: u32,
    buffer_frames: u32,
) -> Result<WasapiSharedInitialization, &'static str> {
    if endpoint_id.is_empty() || requested_period_frames == 0 || buffer_frames == 0 {
        return Err("WASAPI returned invalid shared initialization evidence");
    }
    Ok(WasapiSharedInitialization {
        endpoint_id,
        requested_period_frames,
        buffer_frames,
    })
}

fn requires_render_direction(direction: Direction) -> Result<(), &'static str> {
    if direction == Direction::Output {
        Ok(())
    } else {
        Err("WASAPI endpoint is not a render endpoint")
    }
}

fn wasapi_shared_output_test(
    endpoint_id: String,
    requested_period_frames: u32,
    buffer_frames: u32,
    event_notifications: u32,
    silent_frames_submitted: u64,
    elapsed_ms: u64,
) -> Result<WasapiSharedOutputTest, &'static str> {
    if endpoint_id.is_empty()
        || requested_period_frames == 0
        || buffer_frames == 0
        || event_notifications == 0
        || silent_frames_submitted == 0
        || elapsed_ms < 1_000
    {
        return Err("WASAPI returned invalid shared output evidence");
    }
    Ok(WasapiSharedOutputTest {
        endpoint_id,
        requested_period_frames,
        buffer_frames,
        event_notifications,
        silent_frames_submitted,
        elapsed_ms,
    })
}

fn wasapi_capabilities(
    endpoint_id: String,
    channels: u16,
    sample_rate_hz: u32,
    bits_per_sample: u16,
    periods: (i64, i64, SharedModeEnginePeriods),
    exclusive_shared_mix_supported: bool,
    exclusive_shared_format_sample_rates_hz: Vec<u32>,
) -> Result<WasapiCapabilities, &'static str> {
    let (default_period_100ns, minimum_period_100ns, shared_mode_engine_periods) = periods;
    let rates_are_ordered_and_official = exclusive_shared_format_sample_rates_hz
        .iter()
        .all(|rate| OFFICIAL_SAMPLE_RATES_HZ.contains(rate))
        && exclusive_shared_format_sample_rates_hz
            .windows(2)
            .all(|rates| rates[0] < rates[1]);
    let exact_result_is_consistent = !OFFICIAL_SAMPLE_RATES_HZ.contains(&sample_rate_hz)
        || exclusive_shared_format_sample_rates_hz.contains(&sample_rate_hz)
            == exclusive_shared_mix_supported;
    if endpoint_id.is_empty()
        || channels == 0
        || sample_rate_hz == 0
        || bits_per_sample == 0
        || default_period_100ns < 0
        || minimum_period_100ns < 0
        || minimum_period_100ns > default_period_100ns
        || !rates_are_ordered_and_official
        || !exact_result_is_consistent
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
        exclusive_shared_format_sample_rates_hz,
        shared_mode_engine_periods,
    })
}

fn shared_mode_engine_periods(
    default_frames: u32,
    fundamental_frames: u32,
    minimum_frames: u32,
    maximum_frames: u32,
) -> Result<SharedModeEnginePeriods, &'static str> {
    if fundamental_frames == 0
        || minimum_frames == 0
        || minimum_frames > default_frames
        || default_frames > maximum_frames
        || !minimum_frames.is_multiple_of(fundamental_frames)
        || !default_frames.is_multiple_of(fundamental_frames)
        || !maximum_frames.is_multiple_of(fundamental_frames)
    {
        return Err("WASAPI returned invalid shared engine periods");
    }
    Ok(SharedModeEnginePeriods {
        default_frames,
        fundamental_frames,
        minimum_frames,
        maximum_frames,
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

    fn periods(default_100ns: i64, minimum_100ns: i64) -> (i64, i64, SharedModeEnginePeriods) {
        (
            default_100ns,
            minimum_100ns,
            shared_mode_engine_periods(480, 4, 48, 480).unwrap(),
        )
    }

    #[test]
    fn shared_periods_validate_range_and_fundamental_alignment() {
        let periods = shared_mode_engine_periods(480, 4, 48, 480).unwrap();
        assert_eq!(
            periods,
            SharedModeEnginePeriods {
                default_frames: 480,
                fundamental_frames: 4,
                minimum_frames: 48,
                maximum_frames: 480,
            }
        );
        for supported in [48, 128, 480] {
            assert!(periods.supports_period_frames(supported));
        }
        for unsupported in [0, 44, 49, 481] {
            assert!(!periods.supports_period_frames(unsupported));
        }

        for malformed in [
            (0, 4, 48, 480),
            (480, 0, 48, 480),
            (480, 4, 0, 480),
            (480, 4, 48, 0),
            (480, 4, 484, 480),
            (44, 4, 48, 480),
            (484, 4, 48, 480),
            (478, 4, 48, 480),
            (480, 4, 50, 480),
            (480, 4, 48, 482),
            (480, 7, 48, 480),
        ] {
            assert!(
                shared_mode_engine_periods(malformed.0, malformed.1, malformed.2, malformed.3)
                    .is_err()
            );
        }
    }

    #[test]
    fn shared_initialization_prefers_an_aligned_target_and_rejects_empty_evidence() {
        let aligned = shared_mode_engine_periods(480, 4, 48, 480).unwrap();
        assert_eq!(select_shared_period(&aligned), 128);

        let misaligned = shared_mode_engine_periods(480, 96, 96, 480).unwrap();
        assert_eq!(select_shared_period(&misaligned), 480);

        let out_of_range = shared_mode_engine_periods(480, 32, 160, 480).unwrap();
        assert_eq!(select_shared_period(&out_of_range), 480);

        assert_eq!(
            wasapi_shared_initialization("endpoint-1".into(), 128, 480).unwrap(),
            WasapiSharedInitialization {
                endpoint_id: "endpoint-1".into(),
                requested_period_frames: 128,
                buffer_frames: 480,
            }
        );
        for malformed in [
            wasapi_shared_initialization(String::new(), 128, 480),
            wasapi_shared_initialization("endpoint-1".into(), 0, 480),
            wasapi_shared_initialization("endpoint-1".into(), 128, 0),
        ] {
            assert!(malformed.is_err());
        }
    }

    #[test]
    fn shared_output_test_requires_render_direction() {
        assert!(requires_render_direction(Direction::Output).is_ok());
        assert!(requires_render_direction(Direction::Input).is_err());
    }

    #[test]
    fn shared_output_test_requires_complete_one_second_evidence() {
        let valid = || wasapi_shared_output_test("endpoint-1".into(), 128, 480, 100, 48_480, 1_000);
        assert_eq!(
            valid().unwrap(),
            WasapiSharedOutputTest {
                endpoint_id: "endpoint-1".into(),
                requested_period_frames: 128,
                buffer_frames: 480,
                event_notifications: 100,
                silent_frames_submitted: 48_480,
                elapsed_ms: 1_000,
            }
        );

        for malformed in [
            wasapi_shared_output_test(String::new(), 128, 480, 100, 48_480, 1_000),
            wasapi_shared_output_test("endpoint-1".into(), 0, 480, 100, 48_480, 1_000),
            wasapi_shared_output_test("endpoint-1".into(), 128, 0, 100, 48_480, 1_000),
            wasapi_shared_output_test("endpoint-1".into(), 128, 480, 0, 48_480, 1_000),
            wasapi_shared_output_test("endpoint-1".into(), 128, 480, 100, 0, 1_000),
            wasapi_shared_output_test("endpoint-1".into(), 128, 480, 100, 48_480, 999),
        ] {
            assert!(malformed.is_err());
        }
    }

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
            wasapi_capabilities(
                "endpoint-1".into(),
                8,
                48_000,
                32,
                periods(100_000, 30_000),
                true,
                vec![44_100, 48_000],
            )
            .unwrap(),
            WasapiCapabilities {
                endpoint_id: "endpoint-1".into(),
                channels: 8,
                sample_rate_hz: 48_000,
                bits_per_sample: 32,
                default_period_100ns: 100_000,
                minimum_period_100ns: 30_000,
                exclusive_shared_mix_supported: true,
                exclusive_shared_format_sample_rates_hz: vec![44_100, 48_000],
                shared_mode_engine_periods: shared_mode_engine_periods(480, 4, 48, 480).unwrap(),
            }
        );

        for malformed in [
            wasapi_capabilities("id".into(), 0, 48_000, 32, periods(1, 1), false, vec![]),
            wasapi_capabilities("id".into(), 2, 0, 32, periods(1, 1), false, vec![]),
            wasapi_capabilities("id".into(), 2, 48_000, 0, periods(1, 1), false, vec![]),
            wasapi_capabilities("id".into(), 2, 48_000, 32, periods(-1, 1), false, vec![]),
            wasapi_capabilities("id".into(), 2, 48_000, 32, periods(1, -1), false, vec![]),
            wasapi_capabilities("id".into(), 2, 48_000, 32, periods(1, 2), false, vec![]),
            wasapi_capabilities("id".into(), 2, 48_000, 32, periods(1, 1), true, vec![]),
            wasapi_capabilities(
                "id".into(),
                2,
                48_000,
                32,
                periods(1, 1),
                false,
                vec![48_000],
            ),
            wasapi_capabilities(
                "id".into(),
                2,
                48_000,
                32,
                periods(1, 1),
                true,
                vec![48_000, 44_100],
            ),
            wasapi_capabilities(
                "id".into(),
                2,
                48_000,
                32,
                periods(1, 1),
                true,
                vec![44_100, 44_100, 48_000],
            ),
            wasapi_capabilities(
                "id".into(),
                2,
                48_000,
                32,
                periods(1, 1),
                true,
                vec![22_050, 48_000],
            ),
        ] {
            assert!(malformed.is_err());
        }
    }
}
