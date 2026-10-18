mod common;

use autodj_live_protocol::{EngineCommand, EngineEvent};
use common::*;
use tokio::time::{Duration, sleep, timeout};

#[tokio::test]
async fn detach_reattach_preserves_identity_then_expiry_notifies_observers() {
    let engine = Engine::start(Some(250)).await;
    let mut client = engine.client().await;
    assert_eq!(
        request(
            &mut client,
            EngineCommand::Attach {
                supervisor_generation: 1
            }
        )
        .await,
        EngineEvent::Attached
    );
    assert!(!health(&mut client).await.orphaned);
    assert!(
        matches!(request(&mut client, EngineCommand::Detach).await, EngineEvent::Health(snapshot) if snapshot.orphaned)
    );
    assert!(health(&mut client).await.orphaned);
    assert!(matches!(
        request(&mut client, EngineCommand::Detach).await,
        EngineEvent::Error { .. }
    ));
    assert_eq!(
        request(
            &mut client,
            EngineCommand::Attach {
                supervisor_generation: 2
            }
        )
        .await,
        EngineEvent::Attached
    );
    let snapshot = health(&mut client).await;
    assert_eq!(
        snapshot.engine_id.to_string(),
        engine.descriptor["engine_id"]
    );
    assert_eq!(snapshot.supervisor_generation, 2);
    assert_eq!(snapshot.lease_deadline_ns, 0);
    sleep(Duration::from_millis(300)).await;
    assert!(!health(&mut client).await.orphaned);
    assert!(
        matches!(request(&mut client, EngineCommand::Detach).await, EngineEvent::Health(snapshot) if snapshot.orphaned)
    );
    assert_eq!(
        receive::<EngineEvent>(&mut client).await,
        EngineEvent::Stopping
    );
    engine.finish().await;
}

#[tokio::test]
async fn higher_generation_replaces_owner_and_stale_connections_cannot_mutate_it() {
    let engine = Engine::start(Some(250)).await;
    let mut first = engine.client().await;
    let mut observer = engine.client().await;
    assert!(matches!(
        request(
            &mut first,
            EngineCommand::Attach {
                supervisor_generation: 0
            }
        )
        .await,
        EngineEvent::Error { .. }
    ));
    assert_eq!(
        request(
            &mut first,
            EngineCommand::Attach {
                supervisor_generation: 1
            }
        )
        .await,
        EngineEvent::Attached
    );
    for generation in [0, 1] {
        assert!(matches!(
            request(
                &mut observer,
                EngineCommand::Attach {
                    supervisor_generation: generation
                }
            )
            .await,
            EngineEvent::Error { .. }
        ));
    }
    assert!(matches!(
        request(&mut observer, EngineCommand::Detach).await,
        EngineEvent::Error { .. }
    ));
    assert!(matches!(
        request(&mut observer, EngineCommand::Shutdown).await,
        EngineEvent::Error { .. }
    ));
    assert_eq!(
        request(
            &mut observer,
            EngineCommand::Attach {
                supervisor_generation: 2
            }
        )
        .await,
        EngineEvent::Attached
    );
    assert!(matches!(
        receive::<EngineEvent>(&mut first).await,
        EngineEvent::Error { .. }
    ));
    drop(first);
    sleep(Duration::from_millis(300)).await;
    let snapshot = health(&mut observer).await;
    assert!(!snapshot.orphaned);
    assert_eq!(snapshot.supervisor_generation, 2);
    assert_eq!(
        request(&mut observer, EngineCommand::Shutdown).await,
        EngineEvent::Stopping
    );
    engine.finish().await;
}

#[tokio::test]
async fn owner_disconnect_starts_lease_and_health_does_not_extend_it() {
    let engine = Engine::start(Some(250)).await;
    let mut owner = engine.client().await;
    let mut observer = engine.client().await;
    assert_eq!(
        request(
            &mut owner,
            EngineCommand::Attach {
                supervisor_generation: 1
            }
        )
        .await,
        EngineEvent::Attached
    );
    drop(owner);
    let snapshot = timeout(LIMIT, async {
        loop {
            let snapshot = health(&mut observer).await;
            if snapshot.orphaned {
                break snapshot;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(snapshot.lease_deadline_ns > snapshot.started_ns);
    sleep(Duration::from_millis(50)).await;
    assert_eq!(
        health(&mut observer).await.lease_deadline_ns,
        snapshot.lease_deadline_ns
    );
    assert_eq!(
        receive::<EngineEvent>(&mut observer).await,
        EngineEvent::Stopping
    );
    engine.finish().await;
}
