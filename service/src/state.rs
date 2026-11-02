/// state.rs — state machines for radio, visibility, and transfers

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ── Radio state ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioState {
    /// No lease held; owlyd radio is idle
    Idle,
    /// Acquiring lease from owlyd
    Acquiring,
    /// Radio active; awdl0 is up
    Active,
    /// Releasing lease; Wi-Fi restoring
    Releasing,
    /// owlyd reported an error
    Error(String),
}

impl RadioState {
    pub fn as_str(&self) -> &str {
        match self {
            RadioState::Idle => "idle",
            RadioState::Acquiring => "acquiring",
            RadioState::Active => "active",
            RadioState::Releasing => "releasing",
            RadioState::Error(_) => "error",
        }
    }
}

// ── Visibility (receiving) state ───────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceivingMode {
    Off,
    Timed,
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisibilityState {
    pub mode: ReceivingMode,
    /// Unix timestamp of expiry (for Timed mode), 0 otherwise
    pub expiry: u64,
    /// True when the receive process is actually running
    pub active: bool,
}

impl Default for VisibilityState {
    fn default() -> Self {
        VisibilityState {
            mode: ReceivingMode::Off,
            expiry: 0,
            active: false,
        }
    }
}

// ── Transfer state ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferStatus {
    /// Waiting to be sent / queued
    Queued,
    /// Send: discovering recipient; Receive: offer presented to user
    AwaitingApproval,
    /// Transfer data flowing
    Transferring,
    /// Transfer done, writing to destination
    Finalizing,
    /// Success
    Completed,
    /// Rejected by remote peer or user
    Rejected,
    /// Error occurred
    Failed(String),
    /// Cancelled by local user
    Cancelled,
}

impl TransferStatus {
    pub fn as_str(&self) -> &str {
        match self {
            TransferStatus::Queued => "queued",
            TransferStatus::AwaitingApproval => "awaiting_approval",
            TransferStatus::Transferring => "transferring",
            TransferStatus::Finalizing => "finalizing",
            TransferStatus::Completed => "completed",
            TransferStatus::Rejected => "rejected",
            TransferStatus::Failed(_) => "failed",
            TransferStatus::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TransferStatus::Completed
                | TransferStatus::Rejected
                | TransferStatus::Failed(_)
                | TransferStatus::Cancelled
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub id: u32,
    pub direction: TransferDirection,
    pub status: TransferStatus,
    /// Peer display name (untrusted; used only for UI display)
    pub peer_name: String,
    /// Opaque peer identity (MAC or service ID)
    pub peer_id: String,
    /// File names being transferred (no paths for incoming until accepted)
    pub file_names: Vec<String>,
    /// Total bytes expected (0 if unknown)
    pub total_bytes: u64,
    /// Bytes transferred so far
    pub transferred_bytes: u64,
    /// Destination directory (for incoming, after acceptance)
    pub dest_dir: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
}

impl Transfer {
    pub fn progress_fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.transferred_bytes as f64 / self.total_bytes as f64).min(1.0)
    }
}

// ── Peer ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    /// Opaque unique identity (service record ID from mDNS or AWDL MAC)
    pub id: String,
    /// Display name (untrusted)
    pub name: String,
    /// AWDL MAC address
    pub mac: String,
    /// Device type hint from mDNS flags ("phone", "laptop", "tablet", "unknown")
    pub device_type: String,
    /// When we last saw this peer (for staleness detection)
    #[serde(skip)]
    pub last_seen: Option<Instant>,
}

// ── Incoming offer ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingOffer {
    pub offer_id: u32,
    /// Sender display name (untrusted)
    pub sender_name: String,
    /// File names in the transfer
    pub file_names: Vec<String>,
    /// Total size in bytes (0 if unknown)
    pub total_bytes: u64,
}

// ── Global bridge state ────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct BridgeState {
    pub radio: RadioState,
    pub visibility: VisibilityState,
    pub peers: HashMap<String, Peer>,
    pub transfers: HashMap<u32, Transfer>,
    pub pending_offers: HashMap<u32, IncomingOffer>,
    pub lease_id: u64,
    pub awdl_interface: Option<String>,
    pub next_transfer_id: u32,
    pub next_offer_id: u32,
    pub probe_cache: Option<serde_json::Value>,
}

impl Default for BridgeState {
    fn default() -> Self {
        BridgeState {
            radio: RadioState::Idle,
            visibility: VisibilityState::default(),
            peers: HashMap::new(),
            transfers: HashMap::new(),
            pending_offers: HashMap::new(),
            lease_id: 0,
            awdl_interface: None,
            next_transfer_id: 1,
            next_offer_id: 1,
            probe_cache: None,
        }
    }
}

impl BridgeState {
    pub fn alloc_transfer_id(&mut self) -> u32 {
        let id = self.next_transfer_id;
        self.next_transfer_id += 1;
        id
    }

    pub fn alloc_offer_id(&mut self) -> u32 {
        let id = self.next_offer_id;
        self.next_offer_id += 1;
        id
    }

    /// Expire peers not seen recently (>30s)
    pub fn expire_peers(&mut self) -> Vec<String> {
        let now = Instant::now();
        let stale: Vec<String> = self
            .peers
            .iter()
            .filter(|(_, p)| {
                p.last_seen
                    .map(|t| now.duration_since(t) > Duration::from_secs(30))
                    .unwrap_or(false)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &stale {
            self.peers.remove(id);
        }
        stale
    }
}
