use std::fmt::Debug;

use autodj_live_protocol::{
    BearerToken, ClientHello, EngineCommand, EngineEvent, FrameCodec, HealthSnapshot,
    ProtocolError, ProtocolVersion, ServerHello,
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
    ];
    for event in events {
        assert_framed_round_trip(&event).await;
    }
}

#[tokio::test]
async fn read_rejects_zero_and_oversized_lengths_before_reading_payload() {
    for length in [0_u32, FrameCodec::MAX_FRAME_LEN as u32 + 1] {
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
async fn write_rejects_payloads_over_one_mebibyte() {
    let oversized = vec![0_u8; FrameCodec::MAX_FRAME_LEN + 1];
    let error = FrameCodec::write(&mut Vec::new(), &oversized)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        ProtocolError::FrameTooLarge { length }
            if length > FrameCodec::MAX_FRAME_LEN
    ));
}
