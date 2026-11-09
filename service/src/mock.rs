/// mock.rs — mock backend for UI testing without real hardware
///
/// Activated via AIRDROP_MOCK=1 environment variable.
/// Exercises the same D-Bus contract as the real backend.

use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::info;

use crate::state::{IncomingOffer, Peer};

/// Events that the mock backend emits to the bridge event loop
#[derive(Debug)]
pub enum MockEvent {
    PeerAdded(Peer),
    PeerRemoved(String),
    IncomingOffer(IncomingOffer),
    TransferProgress { id: u32, sent: u64, total: u64 },
    TransferDone(u32),
    RadioUp,
    RadioDown,
}

/// Run the mock backend — simulates peer discovery and an incoming file offer
pub async fn run_mock(tx: mpsc::Sender<MockEvent>) {
    info!("mock backend starting");

    // Simulate radio coming up
    sleep(Duration::from_millis(500)).await;
    let _ = tx.send(MockEvent::RadioUp).await;

    // Simulate peer discovery after 1s
    sleep(Duration::from_secs(1)).await;
    let _ = tx
        .send(MockEvent::PeerAdded(Peer {
            id: "mock-peer-iphone".into(),
            name: "Alice's iPhone".into(),
            mac: "aa:bb:cc:dd:ee:ff".into(),
            device_type: "phone".into(),
            last_seen: Some(std::time::Instant::now()),
        }))
        .await;

    sleep(Duration::from_millis(800)).await;
    let _ = tx
        .send(MockEvent::PeerAdded(Peer {
            id: "mock-peer-mac".into(),
            name: "Bob's MacBook Pro".into(),
            mac: "11:22:33:44:55:66".into(),
            device_type: "laptop".into(),
            last_seen: Some(std::time::Instant::now()),
        }))
        .await;

    // Simulate an incoming file offer after 5s
    sleep(Duration::from_secs(5)).await;
    let _ = tx
        .send(MockEvent::IncomingOffer(IncomingOffer {
            offer_id: 1,
            sender_name: "Alice's iPhone".into(),
            file_names: vec!["photo.jpg".into()],
            total_bytes: 2_400_000,
        }))
        .await;

    // Simulate transfer progress (if accepted, the bridge drives this)
    // In mock mode we just simulate it after 3s
    sleep(Duration::from_secs(3)).await;
    let steps = 10u32;
    let total = 2_400_000u64;
    for i in 1..=steps {
        let sent = total * i as u64 / steps as u64;
        let _ = tx
            .send(MockEvent::TransferProgress {
                id: 1,
                sent,
                total,
            })
            .await;
        sleep(Duration::from_millis(800)).await;
    }
    let _ = tx.send(MockEvent::TransferDone(1)).await;

    // Remove a peer after a while
    sleep(Duration::from_secs(5)).await;
    let _ = tx.send(MockEvent::PeerRemoved("mock-peer-mac".into())).await;
}
