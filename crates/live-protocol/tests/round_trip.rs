use std::fmt::Debug;

use autodj_live_protocol::{
    AsioRegistration, AudioDirection, AudioDiscovery, AudioEndpoint, AudioEndpointState,
    BearerToken, ClientHello, EngineCommand, EngineEvent, FrameCodec, HealthSnapshot,
    ProtocolError, ProtocolVersion, ServerHello, WasapiCapabilities,
};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

fn health_snapshot() -> HealthSnapshot {
    HealthSnapshot {
        engine_id: Uuid::from_u128(0x0102030405060708090a0b0c0d0e0f10),
        state: "attached".to_owned(),
        orphaned: false,
        supervisor_generation: 7,
        started_ns: 1_000,
        lease_deadline_ns: 31_000_000_000,
        extensions: Default::default(),
    }
}

async fn assert_framed_round_trip<T>(message: &T)
where
    T: Debug + DeserializeOwned + PartialEq + Serialize,
{
    let mut frame = Vec::new();
    FrameCodec::write(&mut frame, message).await.unwrap();

    let payload_len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    assert_eq!(payload_len, frame.len() - 4);
    assert_ne!(payload_len, 0);

    let decoded: T = rmp_serde::from_slice(&frame[4..]).unwrap();
    assert_eq!(&decoded, message);
}

#[tokio::test]
async fn foundation_messages_round_trip_through_big_endian_frames() {
    let version = ProtocolVersion::CURRENT;
    let session_id = Uuid::from_u128(0x11111111222233334444555555555555);
    let engine_id = Uuid::from_u128(0xaaaaaaaa_bbbb_cccc_dddd_eeeeeeeeeeee);
    let health = health_snapshot();

    assert_framed_round_trip(&ClientHello {
        version,
        session_id,
        token: BearerToken::new([0x5a; 32]),
    })
    .await;
    assert_framed_round_trip(&ServerHello {
        version,
        engine_id,
        pid: 42,
        lease_deadline_ns: 31_000_000_000,
    })
    .await;
    assert_framed_round_trip(&health).await;

    let commands = [
        EngineCommand::Health,
        EngineCommand::DiscoverAudio,
        EngineCommand::ProbeWasapi {
            endpoint_id: "endpoint-1".into(),
        },
        EngineCommand::Attach {
            supervisor_generation: 7,
        },
        EngineCommand::Detach,
        EngineCommand::Shutdown,
    ];
    for command in commands {
        assert_framed_round_trip(&command).await;

        let mut frame = Vec::new();
        FrameCodec::write(&mut frame, &command).await.unwrap();
        assert_eq!(
            FrameCodec::read(&mut frame.as_slice()).await.unwrap(),
            command
        );
    }

    let events = [
        EngineEvent::Attached,
        EngineEvent::Health(health),
        EngineEvent::Stopping,
        EngineEvent::Error {
            message: "lease expired".to_owned(),
        },
        EngineEvent::AudioDiscovery(AudioDiscovery {
            endpoints: vec![AudioEndpoint {
                id: "endpoint-1".into(),
                name: "Deck 1 L/R".into(),
                direction: AudioDirection::Output,
                state: AudioEndpointState::Active,
            }],
            asio_drivers: vec![AsioRegistration {
                registration: "Rane PERFORMER ASIO".into(),
                clsid: "{6e9aac96-aeda-4f9a-b222-13ea83facee8}".into(),
            }],
            issues: vec!["stale endpoint name unavailable".into()],
        }),
        EngineEvent::WasapiCapabilities(WasapiCapabilities {
            endpoint_id: "endpoint-1".into(),
            channels: 8,
            sample_rate_hz: 48_000,
            bits_per_sample: 32,
            default_period_100ns: 100_000,
            minimum_period_100ns: 30_000,
        }),
    ];
    for event in events {
        assert_framed_round_trip(&event).await;
    }
}

#[tokio::test]
async fn read_rejects_zero_and_oversized_lengths_before_reading_payload() {
    for length in [0_u32, 1_048_577] {
        let (mut writer, mut reader) = tokio::io::duplex(4);
        writer.write_all(&length.to_be_bytes()).await.unwrap();

        let error = FrameCodec::read(&mut reader).await.unwrap_err();
        assert!(matches!(
            error,
            ProtocolError::InvalidFrameLength { length: actual }
                if actual == length
        ));
    }
}

#[tokio::test]
async fn typed_read_decodes_hello_and_preserves_the_frame_bound() {
    let hello = ClientHello {
        version: ProtocolVersion::CURRENT,
        session_id: Uuid::nil(),
        token: BearerToken::new([0x5a; 32]),
    };
    let mut frame = Vec::new();
    FrameCodec::write(&mut frame, &hello).await.unwrap();
    assert_eq!(
        FrameCodec::read_typed::<_, ClientHello>(&mut frame.as_slice())
            .await
            .unwrap(),
        hello
    );
    for length in [0_u32, 1_048_577] {
        let (mut writer, mut reader) = tokio::io::duplex(4);
        writer.write_all(&length.to_be_bytes()).await.unwrap();
        assert!(
            matches!(FrameCodec::read_typed::<_, ClientHello>(&mut reader).await, Err(ProtocolError::InvalidFrameLength { length: actual }) if actual == length)
        );
    }
}

#[tokio::test]
async fn typed_read_rejects_trailing_bytes_in_hello_payload() {
    let hello = ClientHello {
        version: ProtocolVersion::CURRENT,
        session_id: Uuid::nil(),
        token: BearerToken::new([0x5a; 32]),
    };
    for suffix in [0xc1, 0xc0] {
        let frame = frame_with_suffix(&hello, suffix).await;
        assert!(
            FrameCodec::read_typed::<_, ClientHello>(&mut frame.as_slice())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn command_read_rejects_trailing_bytes_in_command_payload() {
    for suffix in [0xc1, 0xc0] {
        let frame = frame_with_suffix(&EngineCommand::Health, suffix).await;
        assert!(FrameCodec::read(&mut frame.as_slice()).await.is_err());
    }
}

async fn frame_with_suffix<T: Serialize>(message: &T, suffix: u8) -> Vec<u8> {
    let mut frame = Vec::new();
    FrameCodec::write(&mut frame, message).await.unwrap();
    frame.push(suffix);
    let payload_len = u32::try_from(frame.len() - 4).unwrap();
    frame[..4].copy_from_slice(&payload_len.to_be_bytes());
    frame
}

#[tokio::test]
async fn write_accepts_one_mebibyte_and_rejects_one_byte_more() {
    // A MessagePack array32 contributes five bytes before these one-byte zero values.
    let exactly_one_mebibyte = vec![0_u8; 1_048_571];
    let mut frame = Vec::new();
    FrameCodec::write(&mut frame, &exactly_one_mebibyte)
        .await
        .unwrap();
    assert_eq!(frame.len() - 4, 1_048_576);

    let oversized = vec![0_u8; 1_048_572];
    let error = FrameCodec::write(&mut Vec::new(), &oversized)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        ProtocolError::FrameTooLarge { length: 1_048_577 }
    ));
}
