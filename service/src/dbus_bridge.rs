/// dbus_bridge.rs — session D-Bus service: io.github.t4t5.AirDropGnome1

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use zbus::{connection, interface, object_server::SignalContext};

use crate::config::Config;
use crate::mock;
use crate::owly_process;
use crate::owly_radio::RadioClient;
use crate::state::{
    BridgeState, IncomingOffer, Peer, RadioState, ReceivingMode, Transfer,
    TransferDirection, TransferStatus, VisibilityState,
};

pub const API_VERSION: u32 = 1;
pub const BRIDGE_VERSION: &str = env!("CARGO_PKG_VERSION");

type SharedState = Arc<RwLock<BridgeState>>;

#[derive(Debug)]
enum BridgeEvent {
    RadioStateChanged(String, String), // new_state, reason
    PeerAdded(Peer),
    PeerRemoved(String),
    BrowseEvent(owly_process::BrowseEvent),
    SendEvent { transfer_id: u32, event: owly_process::SendEvent },
    ReceiveEvent(owly_process::ReceiveEvent),
    MockEvent(mock::MockEvent),
    SetReceiving { mode: ReceivingMode, duration_secs: u32 },
    BrowseRequest,
    AcceptOffer(u32),
    RejectOffer(u32),
    CancelTransfer(u32),
    StartSend { transfer_id: u32, files: Vec<String>, peer_id: String },
    Shutdown,
}

pub struct AirDropGnomeBridge {
    state: SharedState,
    event_tx: mpsc::UnboundedSender<BridgeEvent>,
}

#[interface(name = "io.github.t4t5.AirDropGnome1")]
impl AirDropGnomeBridge {
    async fn get_version(
        &self,
    ) -> zbus::fdo::Result<(u32, String, HashMap<String, zbus::zvariant::Value<'static>>)> {
        let caps: HashMap<String, zbus::zvariant::Value<'static>> = HashMap::new();
        Ok((API_VERSION, BRIDGE_VERSION.to_string(), caps))
    }

    async fn get_radio_state(
        &self,
    ) -> zbus::fdo::Result<HashMap<String, zbus::zvariant::Value<'static>>> {
        let state = self.state.read().await;
        let mut map: HashMap<String, zbus::zvariant::Value<'static>> = HashMap::new();
        map.insert("radio_state".into(), state.radio.as_str().to_string().into());
        map.insert(
            "receiving_mode".into(),
            match state.visibility.mode {
                ReceivingMode::Off => "off",
                ReceivingMode::Timed => "timed",
                ReceivingMode::Always => "always",
            }
            .to_string()
            .into(),
        );
        map.insert("receiving_expiry".into(), state.visibility.expiry.into());
        map.insert("receiving_active".into(), state.visibility.active.into());
        map.insert(
            "awdl_interface".into(),
            state
                .awdl_interface
                .clone()
                .unwrap_or_default()
                .into(),
        );
        Ok(map)
    }

    async fn set_receiving(
        &self,
        mode: String,
        duration_secs: u32,
    ) -> zbus::fdo::Result<()> {
        let recv_mode = match mode.as_str() {
            "off" => ReceivingMode::Off,
            "timed" => ReceivingMode::Timed,
            "always" => ReceivingMode::Always,
            _ => return Err(zbus::fdo::Error::InvalidArgs(
                format!("Unknown mode: {mode}; use 'off', 'timed', or 'always'"),
            )),
        };
        let _ = self
            .event_tx
            .send(BridgeEvent::SetReceiving {
                mode: recv_mode,
                duration_secs,
            });
        Ok(())
    }

    async fn browse(&self) -> zbus::fdo::Result<()> {
        let _ = self.event_tx.send(BridgeEvent::BrowseRequest);
        Ok(())
    }

    async fn send_files(
        &self,
        files: Vec<String>,
        peer_id: String,
    ) -> zbus::fdo::Result<u32> {
        for f in &files {
            if !std::path::Path::new(f).exists() {
                return Err(zbus::fdo::Error::InvalidArgs(format!("File not found: {f}")));
            }
        }

        let transfer_id = {
            let mut state = self.state.write().await;
            state.alloc_transfer_id()
        };

        let _ = self
            .event_tx
            .send(BridgeEvent::StartSend {
                transfer_id,
                files,
                peer_id,
            });

        Ok(transfer_id)
    }

    async fn cancel_transfer(&self, transfer_id: u32) -> zbus::fdo::Result<()> {
        let _ = self.event_tx.send(BridgeEvent::CancelTransfer(transfer_id));
        Ok(())
    }

    async fn accept_incoming(&self, offer_id: u32) -> zbus::fdo::Result<()> {
        let _ = self.event_tx.send(BridgeEvent::AcceptOffer(offer_id));
        Ok(())
    }

    async fn reject_incoming(&self, offer_id: u32) -> zbus::fdo::Result<()> {
        let _ = self.event_tx.send(BridgeEvent::RejectOffer(offer_id));
        Ok(())
    }

    async fn get_transfers(
        &self,
    ) -> zbus::fdo::Result<Vec<HashMap<String, zbus::zvariant::Value<'static>>>> {
        let state = self.state.read().await;
        Ok(state
            .transfers
            .values()
            .map(transfer_to_dict)
            .collect())
    }

    async fn get_peers(
        &self,
    ) -> zbus::fdo::Result<Vec<HashMap<String, zbus::zvariant::Value<'static>>>> {
        let state = self.state.read().await;
        Ok(state.peers.values().map(peer_to_dict).collect())
    }

    async fn get_diagnostics(&self) -> zbus::fdo::Result<String> {
        let state = self.state.read().await;
        if let Some(probe) = &state.probe_cache {
            Ok(probe.to_string())
        } else {
            Ok("{\"status\":\"not_probed\"}".to_string())
        }
    }

    // ── Signals ──────────────────────────────────────────────────────────────

    #[zbus(signal)]
    async fn notify_radio_state_changed(
        ctxt: &SignalContext<'_>,
        state: HashMap<String, zbus::zvariant::Value<'static>>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notify_peer_added(
        ctxt: &SignalContext<'_>,
        peer: HashMap<String, zbus::zvariant::Value<'static>>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notify_peer_removed(ctxt: &SignalContext<'_>, peer_id: String) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notify_peer_changed(
        ctxt: &SignalContext<'_>,
        peer: HashMap<String, zbus::zvariant::Value<'static>>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notify_incoming_offer(
        ctxt: &SignalContext<'_>,
        offer: HashMap<String, zbus::zvariant::Value<'static>>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notify_transfer_progress(
        ctxt: &SignalContext<'_>,
        transfer: HashMap<String, zbus::zvariant::Value<'static>>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notify_transfer_completed(
        ctxt: &SignalContext<'_>,
        transfer: HashMap<String, zbus::zvariant::Value<'static>>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notify_transfer_failed(
        ctxt: &SignalContext<'_>,
        transfer: HashMap<String, zbus::zvariant::Value<'static>>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notify_transfer_cancelled(
        ctxt: &SignalContext<'_>,
        transfer_id: u32,
    ) -> zbus::Result<()>;

    // ── Properties ───────────────────────────────────────────────────────────

    #[zbus(property)]
    async fn api_version(&self) -> u32 {
        API_VERSION
    }

    #[zbus(property)]
    async fn current_radio_state(&self) -> String {
        self.state.read().await.radio.as_str().to_string()
    }

    #[zbus(property)]
    async fn receiving_mode(&self) -> String {
        match self.state.read().await.visibility.mode {
            ReceivingMode::Off => "off".into(),
            ReceivingMode::Timed => "timed".into(),
            ReceivingMode::Always => "always".into(),
        }
    }

    #[zbus(property)]
    async fn receiving_expiry(&self) -> u64 {
        self.state.read().await.visibility.expiry
    }

    #[zbus(property)]
    async fn backend_available(&self) -> bool {
        true
    }

    #[zbus(property)]
    async fn adapter_ready(&self) -> bool {
        self.state
            .read()
            .await
            .probe_cache
            .as_ref()
            .and_then(|p| p.get("hardware"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

fn peer_to_dict(peer: &Peer) -> HashMap<String, zbus::zvariant::Value<'static>> {
    let mut m: HashMap<String, zbus::zvariant::Value<'static>> = HashMap::new();
    m.insert("id".into(), peer.id.clone().into());
    m.insert("name".into(), peer.name.clone().into());
    m.insert("mac".into(), peer.mac.clone().into());
    m.insert("device_type".into(), peer.device_type.clone().into());
    m
}

fn transfer_to_dict(t: &Transfer) -> HashMap<String, zbus::zvariant::Value<'static>> {
    let mut m: HashMap<String, zbus::zvariant::Value<'static>> = HashMap::new();
    m.insert("id".into(), t.id.into());
    m.insert(
        "direction".into(),
        match t.direction {
            TransferDirection::Outgoing => "outgoing",
            TransferDirection::Incoming => "incoming",
        }
        .to_string()
        .into(),
    );
    m.insert("status".into(), t.status.as_str().to_string().into());
    m.insert("peer_name".into(), t.peer_name.clone().into());
    m.insert("peer_id".into(), t.peer_id.clone().into());
    m.insert("file_names".into(), t.file_names.join(",").into());
    m.insert("total_bytes".into(), t.total_bytes.into());
    m.insert("transferred_bytes".into(), t.transferred_bytes.into());
    m.insert(
        "dest_dir".into(),
        t.dest_dir.clone().unwrap_or_default().into(),
    );
    m.insert("error".into(), t.error.clone().unwrap_or_default().into());
    m
}

fn offer_to_dict(o: &IncomingOffer) -> HashMap<String, zbus::zvariant::Value<'static>> {
    let mut m: HashMap<String, zbus::zvariant::Value<'static>> = HashMap::new();
    m.insert("offer_id".into(), o.offer_id.into());
    m.insert("sender_name".into(), o.sender_name.clone().into());
    m.insert("file_names".into(), o.file_names.join(",").into());
    m.insert("total_bytes".into(), o.total_bytes.into());
    m
}

pub async fn run(cfg: Config, mock_mode: bool) -> Result<()> {
    let state: SharedState = Arc::new(RwLock::new(BridgeState::default()));
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<BridgeEvent>();

    let bridge_obj = AirDropGnomeBridge {
        state: state.clone(),
        event_tx: ev_tx.clone(),
    };

    let session_conn = connection::Builder::session()?
        .name("io.github.t4t5.AirDropGnome")?
        .serve_at("/io/github/t4t5/AirDropGnome", bridge_obj)?
        .build()
        .await?;

    info!("session D-Bus service registered: io.github.t4t5.AirDropGnome");

    let iface_ref = session_conn
        .object_server()
        .interface::<_, AirDropGnomeBridge>("/io/github/t4t5/AirDropGnome")
        .await?;

    let radio_client = if !mock_mode {
        match RadioClient::connect().await {
            Ok(radio) => {
                let probe_json = radio.probe_json_redacted().await.unwrap_or_default();
                if let Ok(probe_val) = serde_json::from_str::<serde_json::Value>(&probe_json) {
                    state.write().await.probe_cache = Some(probe_val);
                }
                info!("connected to owlyd");

                let ev_tx2 = ev_tx.clone();
                let radio_arc = Arc::new(radio);
                let radio_clone = radio_arc.clone();
                tokio::spawn(async move {
                    let _ = radio_clone
                        .watch_state(move |_lease, new_state, reason| {
                            let _ = ev_tx2.send(BridgeEvent::RadioStateChanged(
                                new_state, reason,
                            ));
                        })
                        .await;
                });
                Some(radio_arc)
            }
            Err(e) => {
                warn!("owlyd not available: {e}; extension will show setup required state");
                None
            }
        }
    } else {
        None
    };

    if mock_mode {
        let (mock_tx, mut mock_rx) = mpsc::channel::<mock::MockEvent>(32);
        let ev_tx2 = ev_tx.clone();
        tokio::spawn(async move {
            mock::run_mock(mock_tx).await;
        });
        tokio::spawn(async move {
            while let Some(ev) = mock_rx.recv().await {
                let _ = ev_tx2.send(BridgeEvent::MockEvent(ev));
            }
        });
    }

    let mut current_lease_id: Option<u64> = None;
    let mut receive_cancel: Option<CancellationToken> = None;
    let mut browse_cancel: Option<CancellationToken> = None;
    let mut send_cancels: HashMap<u32, CancellationToken> = HashMap::new();
    let mut receive_accept_tx: Option<mpsc::Sender<bool>> = None;

    loop {
        let ev = match ev_rx.recv().await {
            Some(ev) => ev,
            None => break,
        };

        match ev {
            BridgeEvent::Shutdown => break,

            BridgeEvent::MockEvent(mock_ev) => {
                handle_mock_event(mock_ev, &state, &iface_ref).await;
            }

            BridgeEvent::RadioStateChanged(new_state, reason) => {
                info!(state = %new_state, reason = %reason, "owlyd state changed");
                {
                    let mut st = state.write().await;
                    st.radio = match new_state.as_str() {
                        "idle" => RadioState::Idle,
                        "starting" => RadioState::Acquiring,
                        "up" => RadioState::Active,
                        "stopping" => RadioState::Releasing,
                        other => RadioState::Error(other.to_string()),
                    };
                }
                emit_radio_state(&iface_ref, &state).await;
            }

            BridgeEvent::SetReceiving { mode, duration_secs } => {
                handle_set_receiving(
                    mode,
                    duration_secs,
                    &cfg,
                    &state,
                    &iface_ref,
                    &ev_tx,
                    &radio_client,
                    &mut current_lease_id,
                    &mut receive_cancel,
                    &mut receive_accept_tx,
                    mock_mode,
                )
                .await;
            }

            BridgeEvent::BrowseRequest => {
                if let Some(cancel) = browse_cancel.take() {
                    cancel.cancel();
                }
                let cancel = CancellationToken::new();
                browse_cancel = Some(cancel.clone());
                let ev_tx_browse = ev_tx.clone();

                // Only browse if the radio is already active (e.g. receiving is on or send is active)
                // Never silently acquire the radio and kill the host network on browsing!
                if current_lease_id.is_none() && !mock_mode {
                    debug!("browse requested while radio is idle; skipping to protect host Wi-Fi");
                    continue;
                }

                let iface_name = state.read().await.awdl_interface.clone().unwrap_or_else(|| "awdl0".into());
                let display = cfg.display_name.clone();

                tokio::spawn(async move {
                    let (btx, mut brx) = mpsc::channel(16);
                    let btx2 = btx.clone();
                    let cancel_inner = cancel.clone();
                    tokio::spawn(async move {
                        let _ = owly_process::browse(&iface_name, 10, display.as_deref(), cancel_inner, btx2).await;
                    });
                    while let Some(bev) = brx.recv().await {
                        let _ = ev_tx_browse.send(BridgeEvent::BrowseEvent(bev));
                    }
                });
            }

            BridgeEvent::PeerAdded(peer) => {
                let dict = peer_to_dict(&peer);
                {
                    let mut st = state.write().await;
                    st.peers.insert(peer.id.clone(), peer);
                }
                let _ = AirDropGnomeBridge::notify_peer_added(iface_ref.signal_context(), dict).await;
            }

            BridgeEvent::PeerRemoved(peer_id) => {
                {
                    state.write().await.peers.remove(&peer_id);
                }
                let _ = AirDropGnomeBridge::notify_peer_removed(iface_ref.signal_context(), peer_id).await;
            }

            BridgeEvent::BrowseEvent(browse_ev) => {
                let is_done = matches!(browse_ev, owly_process::BrowseEvent::Done | owly_process::BrowseEvent::Error(_));
                handle_browse_event(browse_ev, &state, &iface_ref).await;
                if is_done {
                    maybe_release_idle_lease(&state, &radio_client, &mut current_lease_id, &iface_ref).await;
                }
            }

            BridgeEvent::SendEvent { transfer_id, event } => {
                let is_terminal = matches!(event, owly_process::SendEvent::Done | owly_process::SendEvent::Rejected | owly_process::SendEvent::Error(_));
                handle_send_event(transfer_id, event, &state, &iface_ref).await;
                if is_terminal {
                    maybe_release_idle_lease(&state, &radio_client, &mut current_lease_id, &iface_ref).await;
                }
            }

            BridgeEvent::ReceiveEvent(recv_ev) => {
                handle_receive_event(
                    recv_ev,
                    &state,
                    &iface_ref,
                )
                .await;
            }

            BridgeEvent::AcceptOffer(offer_id) => {
                if let Some(tx) = &receive_accept_tx {
                    let _ = tx.send(true).await;
                }
                let st = state.read().await;
                if let Some(offer) = st.pending_offers.get(&offer_id) {
                    info!(offer_id, sender = %offer.sender_name, "offer accepted");
                }
            }

            BridgeEvent::RejectOffer(offer_id) => {
                if let Some(tx) = &receive_accept_tx {
                    let _ = tx.send(false).await;
                }
                state.write().await.pending_offers.remove(&offer_id);
                info!(offer_id, "offer rejected");
            }

            BridgeEvent::CancelTransfer(transfer_id) => {
                if let Some(cancel) = send_cancels.remove(&transfer_id) {
                    cancel.cancel();
                }
                {
                    let mut st = state.write().await;
                    if let Some(t) = st.transfers.get_mut(&transfer_id) {
                        t.status = TransferStatus::Cancelled;
                    }
                }
                let _ = AirDropGnomeBridge::notify_transfer_cancelled(
                    iface_ref.signal_context(),
                    transfer_id,
                )
                .await;
            }

            BridgeEvent::StartSend { transfer_id, files, peer_id } => {
                start_send(
                    transfer_id,
                    files,
                    peer_id,
                    &cfg,
                    &state,
                    &ev_tx,
                    &radio_client,
                    &mut current_lease_id,
                    &mut send_cancels,
                    mock_mode,
                )
                .await;
            }
        }
    }

    if let (Some(radio), Some(lease)) = (radio_client, current_lease_id) {
        let _ = radio.release(lease).await;
    }

    info!("bridge shutting down");
    Ok(())
}

async fn handle_mock_event(
    ev: mock::MockEvent,
    state: &SharedState,
    iface_ref: &zbus::object_server::InterfaceRef<AirDropGnomeBridge>,
) {
    match ev {
        mock::MockEvent::RadioUp => {
            state.write().await.radio = RadioState::Active;
            emit_radio_state(iface_ref, state).await;
        }
        mock::MockEvent::RadioDown => {
            state.write().await.radio = RadioState::Idle;
            emit_radio_state(iface_ref, state).await;
        }
        mock::MockEvent::PeerAdded(peer) => {
            let dict = peer_to_dict(&peer);
            state.write().await.peers.insert(peer.id.clone(), peer);
            let _ = AirDropGnomeBridge::notify_peer_added(iface_ref.signal_context(), dict).await;
        }
        mock::MockEvent::PeerRemoved(id) => {
            state.write().await.peers.remove(&id);
            let _ = AirDropGnomeBridge::notify_peer_removed(iface_ref.signal_context(), id).await;
        }
        mock::MockEvent::IncomingOffer(offer) => {
            let dict = offer_to_dict(&offer);
            state.write().await.pending_offers.insert(offer.offer_id, offer);
            let _ = AirDropGnomeBridge::notify_incoming_offer(iface_ref.signal_context(), dict).await;
        }
        mock::MockEvent::TransferProgress { id, sent, total } => {
            let mut st = state.write().await;
            if let Some(t) = st.transfers.get_mut(&id) {
                t.transferred_bytes = sent;
                t.total_bytes = total;
                t.status = TransferStatus::Transferring;
            }
            drop(st);
            let st = state.read().await;
            if let Some(t) = st.transfers.get(&id) {
                let dict = transfer_to_dict(t);
                let _ =
                    AirDropGnomeBridge::notify_transfer_progress(iface_ref.signal_context(), dict).await;
            }
        }
        mock::MockEvent::TransferDone(id) => {
            let mut st = state.write().await;
            if let Some(t) = st.transfers.get_mut(&id) {
                t.status = TransferStatus::Completed;
                t.transferred_bytes = t.total_bytes;
            }
            drop(st);
            let st = state.read().await;
            if let Some(t) = st.transfers.get(&id) {
                let dict = transfer_to_dict(t);
                let _ =
                    AirDropGnomeBridge::notify_transfer_completed(iface_ref.signal_context(), dict).await;
            }
        }
    }
}

async fn handle_set_receiving(
    mode: ReceivingMode,
    duration_secs: u32,
    cfg: &Config,
    state: &SharedState,
    iface_ref: &zbus::object_server::InterfaceRef<AirDropGnomeBridge>,
    ev_tx: &mpsc::UnboundedSender<BridgeEvent>,
    radio_client: &Option<Arc<RadioClient>>,
    current_lease_id: &mut Option<u64>,
    receive_cancel: &mut Option<CancellationToken>,
    receive_accept_tx: &mut Option<mpsc::Sender<bool>>,
    mock_mode: bool,
) {
    if let Some(cancel) = receive_cancel.take() {
        cancel.cancel();
    }
    *receive_accept_tx = None;

    if mode == ReceivingMode::Off {
        if let (Some(radio), Some(lease)) = (radio_client.as_ref(), current_lease_id.take()) {
            info!(lease, "releasing radio lease for receiving off");
            let _ = radio.release(lease).await;
        }
        {
            let mut st = state.write().await;
            st.visibility = VisibilityState {
                mode: ReceivingMode::Off,
                expiry: 0,
                active: false,
            };
            st.radio = RadioState::Idle;
        }
        emit_radio_state(iface_ref, state).await;
        return;
    }

    if mock_mode {
        let mut st = state.write().await;
        st.visibility = VisibilityState {
            mode,
            expiry: if duration_secs > 0 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + duration_secs as u64 } else { 0 },
            active: true,
        };
        st.radio = RadioState::Active;
        drop(st);
        emit_radio_state(iface_ref, state).await;
        return;
    }

    // Acquire lease from owlyd if not already holding one
    if let Some(radio) = radio_client.as_ref() {
        if current_lease_id.is_none() {
            info!("requesting radio lease from owlyd for receiving...");
            match radio.acquire(0).await {
                Ok((lease, iface, _addr)) => {
                    *current_lease_id = Some(lease);
                    let mut st = state.write().await;
                    st.lease_id = lease;
                    st.awdl_interface = Some(iface);
                    st.radio = RadioState::Active;
                }
                Err(e) => {
                    error!("failed to acquire radio lease: {e}");
                    let mut st = state.write().await;
                    st.radio = RadioState::Error(e.to_string());
                    drop(st);
                    emit_radio_state(iface_ref, state).await;
                    return;
                }
            }
        }
    }

    let expiry = if mode == ReceivingMode::Timed && duration_secs > 0 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + duration_secs as u64
    } else {
        0
    };

    {
        let mut st = state.write().await;
        st.visibility = VisibilityState {
            mode,
            expiry,
            active: true,
        };
    }
    emit_radio_state(iface_ref, state).await;

    let awdl_iface = state
        .read()
        .await
        .awdl_interface
        .clone()
        .unwrap_or_else(|| "awdl0".into());

    let cancel = CancellationToken::new();
    *receive_cancel = Some(cancel.clone());

    let dest_dir = cfg.receive_dir.clone();
    let display_name = cfg.display_name.clone();
    let port = cfg.receive_port;

    match owly_process::start_receive(
        &awdl_iface,
        &dest_dir,
        port,
        display_name.as_deref(),
        cancel,
    )
    .await
    {
        Ok((mut recv_rx, accept_tx)) => {
            *receive_accept_tx = Some(accept_tx);
            let ev_tx2 = ev_tx.clone();
            tokio::spawn(async move {
                while let Some(ev) = recv_rx.recv().await {
                    let _ = ev_tx2.send(BridgeEvent::ReceiveEvent(ev));
                }
            });
        }
        Err(e) => {
            error!("receive process failed: {e}");
        }
    }
}

async fn handle_browse_event(
    ev: owly_process::BrowseEvent,
    state: &SharedState,
    iface_ref: &zbus::object_server::InterfaceRef<AirDropGnomeBridge>,
) {
    match ev {
        owly_process::BrowseEvent::PeerFound(peer) => {
            let is_new = !state.read().await.peers.contains_key(&peer.id);
            let dict = peer_to_dict(&peer);
            state.write().await.peers.insert(peer.id.clone(), peer);
            if is_new {
                let _ = AirDropGnomeBridge::notify_peer_added(iface_ref.signal_context(), dict).await;
            } else {
                let _ = AirDropGnomeBridge::notify_peer_changed(iface_ref.signal_context(), dict).await;
            }
        }
        owly_process::BrowseEvent::Done => {
            debug!("browse scan complete");
        }
        owly_process::BrowseEvent::Error(e) => {
            warn!("browse error: {e}");
        }
    }
}

async fn handle_send_event(
    transfer_id: u32,
    ev: owly_process::SendEvent,
    state: &SharedState,
    iface_ref: &zbus::object_server::InterfaceRef<AirDropGnomeBridge>,
) {
    match ev {
        owly_process::SendEvent::Progress(sent, total) => {
            {
                let mut st = state.write().await;
                if let Some(t) = st.transfers.get_mut(&transfer_id) {
                    t.transferred_bytes = sent;
                    t.total_bytes = total;
                    t.status = TransferStatus::Transferring;
                }
            }
            let st = state.read().await;
            if let Some(t) = st.transfers.get(&transfer_id) {
                let dict = transfer_to_dict(t);
                let _ =
                    AirDropGnomeBridge::notify_transfer_progress(iface_ref.signal_context(), dict).await;
            }
        }
        owly_process::SendEvent::Done => {
            {
                let mut st = state.write().await;
                if let Some(t) = st.transfers.get_mut(&transfer_id) {
                    t.status = TransferStatus::Completed;
                }
            }
            let st = state.read().await;
            if let Some(t) = st.transfers.get(&transfer_id) {
                let dict = transfer_to_dict(t);
                let _ =
                    AirDropGnomeBridge::notify_transfer_completed(iface_ref.signal_context(), dict).await;
            }
        }
        owly_process::SendEvent::Rejected => {
            {
                let mut st = state.write().await;
                if let Some(t) = st.transfers.get_mut(&transfer_id) {
                    t.status = TransferStatus::Rejected;
                }
            }
            let st = state.read().await;
            if let Some(t) = st.transfers.get(&transfer_id) {
                let dict = transfer_to_dict(t);
                let _ =
                    AirDropGnomeBridge::notify_transfer_failed(iface_ref.signal_context(), dict).await;
            }
        }
        owly_process::SendEvent::Error(e) => {
            {
                let mut st = state.write().await;
                if let Some(t) = st.transfers.get_mut(&transfer_id) {
                    t.status = TransferStatus::Failed(e.clone());
                    t.error = Some(e);
                }
            }
            let st = state.read().await;
            if let Some(t) = st.transfers.get(&transfer_id) {
                let dict = transfer_to_dict(t);
                let _ =
                    AirDropGnomeBridge::notify_transfer_failed(iface_ref.signal_context(), dict).await;
            }
        }
    }
}

async fn handle_receive_event(
    ev: owly_process::ReceiveEvent,
    state: &SharedState,
    iface_ref: &zbus::object_server::InterfaceRef<AirDropGnomeBridge>,
) {
    match ev {
        owly_process::ReceiveEvent::Offer(offer) => {
            let dict = offer_to_dict(&offer);
            state
                .write()
                .await
                .pending_offers
                .insert(offer.offer_id, offer);
            let _ =
                AirDropGnomeBridge::notify_incoming_offer(iface_ref.signal_context(), dict).await;
        }
        owly_process::ReceiveEvent::Accepted(offer_id, dest) => {
            debug!(offer_id, dest = %dest, "offer accepted, transfer starting");
            let mut st = state.write().await;
            st.transfers.insert(
                offer_id,
                Transfer {
                    id: offer_id,
                    direction: TransferDirection::Incoming,
                    status: TransferStatus::Transferring,
                    peer_name: "Nearby Device".into(),
                    peer_id: String::new(),
                    file_names: vec![],
                    total_bytes: 0,
                    transferred_bytes: 0,
                    dest_dir: Some(dest),
                    error: None,
                },
            );
        }
        owly_process::ReceiveEvent::Progress(offer_id, sent, total) => {
            let mut st = state.write().await;
            if let Some(t) = st.transfers.get_mut(&offer_id) {
                t.transferred_bytes = sent;
                t.total_bytes = total;
                t.status = TransferStatus::Transferring;
            }
            drop(st);
            let st = state.read().await;
            if let Some(t) = st.transfers.get(&offer_id) {
                let dict = transfer_to_dict(t);
                let _ = AirDropGnomeBridge::notify_transfer_progress(iface_ref.signal_context(), dict).await;
            }
        }
        owly_process::ReceiveEvent::Done(offer_id) => {
            {
                let mut st = state.write().await;
                st.pending_offers.remove(&offer_id);
                if let Some(t) = st.transfers.get_mut(&offer_id) {
                    t.status = TransferStatus::Completed;
                }
            }
            let st = state.read().await;
            if let Some(t) = st.transfers.get(&offer_id) {
                let dict = transfer_to_dict(t);
                let _ = AirDropGnomeBridge::notify_transfer_completed(iface_ref.signal_context(), dict).await;
            }
        }
        owly_process::ReceiveEvent::Error(e) => {
            error!("receive error: {e}");
        }
    }
}

async fn maybe_release_idle_lease(
    state: &SharedState,
    radio_client: &Option<Arc<RadioClient>>,
    current_lease_id: &mut Option<u64>,
    iface_ref: &zbus::object_server::InterfaceRef<AirDropGnomeBridge>,
) {
    let should_release = {
        let st = state.read().await;
        st.visibility.mode == ReceivingMode::Off && st.transfers.values().all(|t| t.status.is_terminal())
    };
    if should_release {
        if let (Some(radio), Some(lease)) = (radio_client.as_ref(), current_lease_id.take()) {
            info!(lease, "releasing idle radio lease to restore normal Wi-Fi");
            let _ = radio.release(lease).await;
            {
                let mut st = state.write().await;
                st.radio = RadioState::Idle;
            }
            emit_radio_state(iface_ref, state).await;
        }
    }
}

async fn start_send(
    transfer_id: u32,
    files: Vec<String>,
    peer_id: String,
    cfg: &Config,
    state: &SharedState,
    ev_tx: &mpsc::UnboundedSender<BridgeEvent>,
    radio_client: &Option<Arc<RadioClient>>,
    current_lease_id: &mut Option<u64>,
    send_cancels: &mut HashMap<u32, CancellationToken>,
    mock_mode: bool,
) {
    let peer_name = state
        .read()
        .await
        .peers
        .get(&peer_id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| peer_id.clone());

    {
        let mut st = state.write().await;
        st.transfers.insert(
            transfer_id,
            Transfer {
                id: transfer_id,
                direction: TransferDirection::Outgoing,
                status: TransferStatus::Queued,
                peer_name: peer_name.clone(),
                peer_id: peer_id.clone(),
                file_names: files
                    .iter()
                    .map(|f| {
                        std::path::Path::new(f)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    })
                    .collect(),
                total_bytes: 0,
                transferred_bytes: 0,
                dest_dir: None,
                error: None,
            },
        );
    }

    let cancel = CancellationToken::new();
    send_cancels.insert(transfer_id, cancel.clone());

    if !mock_mode {
        if let Some(radio) = radio_client.as_ref() {
            if current_lease_id.is_none() {
                if let Ok((lease, iface, _)) = radio.acquire(0).await {
                    *current_lease_id = Some(lease);
                    let mut st = state.write().await;
                    st.lease_id = lease;
                    st.awdl_interface = Some(iface);
                    st.radio = RadioState::Active;
                }
            }
        }
    }

    let awdl_iface = state
        .read()
        .await
        .awdl_interface
        .clone()
        .unwrap_or_else(|| "awdl0".into());

    let ev_tx2 = ev_tx.clone();
    let display_name = cfg.display_name.clone();

    if mock_mode {
        tokio::spawn(async move {
            use tokio::time::sleep;
            let total = 2_400_000u64;
            for i in 1..=10u64 {
                sleep(std::time::Duration::from_millis(500)).await;
                let _ = ev_tx2
                    .send(BridgeEvent::SendEvent {
                        transfer_id,
                        event: owly_process::SendEvent::Progress(total * i / 10, total),
                    });
            }
            let _ = ev_tx2
                .send(BridgeEvent::SendEvent {
                    transfer_id,
                    event: owly_process::SendEvent::Done,
                });
        });
    } else {
        tokio::spawn(async move {
            let (send_tx, mut send_rx) = mpsc::channel(16);
            let _ = owly_process::send_files(
                &awdl_iface,
                &files,
                &peer_id,
                display_name.as_deref(),
                30,
                cancel,
                send_tx,
            )
            .await;
            while let Some(ev) = send_rx.recv().await {
                let _ = ev_tx2
                    .send(BridgeEvent::SendEvent { transfer_id, event: ev });
            }
        });
    }
}

async fn emit_radio_state(
    iface_ref: &zbus::object_server::InterfaceRef<AirDropGnomeBridge>,
    state: &SharedState,
) {
    let st = state.read().await;
    let mut map: HashMap<String, zbus::zvariant::Value<'static>> = HashMap::new();
    map.insert("radio_state".into(), st.radio.as_str().to_string().into());
    map.insert(
        "receiving_mode".into(),
        match st.visibility.mode {
            ReceivingMode::Off => "off",
            ReceivingMode::Timed => "timed",
            ReceivingMode::Always => "always",
        }
        .to_string()
        .into(),
    );
    map.insert("receiving_expiry".into(), st.visibility.expiry.into());
    map.insert("receiving_active".into(), st.visibility.active.into());
    drop(st);

    let _ = AirDropGnomeBridge::notify_radio_state_changed(iface_ref.signal_context(), map).await;
}
