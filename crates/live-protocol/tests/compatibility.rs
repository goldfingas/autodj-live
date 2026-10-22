use autodj_live_protocol::{
    BearerToken, HealthSnapshot, ProtocolError, ProtocolVersion, WasapiCapabilities,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[test]
fn current_and_newer_minor_versions_are_compatible() {
    assert_eq!(ProtocolVersion::CURRENT.major, 1);
    assert_eq!(ProtocolVersion::CURRENT.minor, 0);
    assert!(ProtocolVersion::CURRENT.ensure_compatible().is_ok());
    assert!(
        ProtocolVersion { major: 1, minor: 9 }
            .ensure_compatible()
            .is_ok()
    );
}

#[test]
fn wrong_major_version_is_rejected() {
    let error = ProtocolVersion { major: 2, minor: 0 }
        .ensure_compatible()
        .unwrap_err();

    assert!(matches!(
        error,
        ProtocolError::IncompatibleMajor {
            expected: 1,
            received: 2,
        }
    ));
}

#[test]
fn newer_minor_unknown_fields_survive_health_decode_and_reencode() {
    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct NewerHealthSnapshot {
        engine_id: Uuid,
        state: String,
        orphaned: bool,
        supervisor_generation: u64,
        started_ns: u64,
        lease_deadline_ns: u64,
        future_counter: u32,
    }

    let engine_id = Uuid::from_u128(0x0102030405060708090a0b0c0d0e0f10);
    let encoded = rmp_serde::to_vec_named(&NewerHealthSnapshot {
        engine_id,
        state: "orphaned".to_owned(),
        orphaned: true,
        supervisor_generation: 8,
        started_ns: 1_000,
        lease_deadline_ns: 31_000_000_000,
        future_counter: 99,
    })
    .unwrap();

    let decoded: HealthSnapshot = rmp_serde::from_slice(&encoded).unwrap();
    assert_eq!(decoded.engine_id, engine_id);
    assert_eq!(decoded.state, "orphaned");
    assert!(decoded.orphaned);
    assert_eq!(decoded.supervisor_generation, 8);
    assert_eq!(decoded.started_ns, 1_000);
    assert_eq!(decoded.lease_deadline_ns, 31_000_000_000);

    let reencoded = rmp_serde::to_vec_named(&decoded).unwrap();
    let recovered: NewerHealthSnapshot = rmp_serde::from_slice(&reencoded).unwrap();
    assert_eq!(
        recovered,
        NewerHealthSnapshot {
            engine_id,
            state: "orphaned".to_owned(),
            orphaned: true,
            supervisor_generation: 8,
            started_ns: 1_000,
            lease_deadline_ns: 31_000_000_000,
            future_counter: 99,
        }
    );
}

#[test]
fn capabilities_from_an_older_engine_mark_rate_probe_unavailable() {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct OlderWasapiCapabilities {
        endpoint_id: String,
        channels: u16,
        sample_rate_hz: u32,
        bits_per_sample: u16,
        default_period_100ns: u64,
        minimum_period_100ns: u64,
        exclusive_shared_mix_supported: bool,
    }

    let encoded = rmp_serde::to_vec_named(&OlderWasapiCapabilities {
        endpoint_id: "endpoint-1".into(),
        channels: 2,
        sample_rate_hz: 48_000,
        bits_per_sample: 32,
        default_period_100ns: 100_000,
        minimum_period_100ns: 30_000,
        exclusive_shared_mix_supported: false,
    })
    .unwrap();

    let decoded: WasapiCapabilities = rmp_serde::from_slice(&encoded).unwrap();
    assert!(decoded.exclusive_shared_format_sample_rates_hz.is_none());
    assert!(decoded.shared_mode_engine_periods.is_none());
}

#[test]
fn bearer_token_debug_output_is_redacted() {
    let token = BearerToken::new([0xab; 32]);
    let output = format!("{token:?}");

    assert_eq!(token.as_bytes(), &[0xab; 32]);
    assert_eq!(output, "BearerToken([REDACTED])");
    assert!(!output.contains("ab"));
}
