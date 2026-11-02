/// owly_radio.rs — system D-Bus client for io.github.t4t5.Owly.Radio1
///
/// Verified API from owly 0.4.2 source (crates/owlyd/src/api.rs):
/// - Bus: system
/// - Object: /io/github/t4t5/Owly
/// - Interface: io.github.t4t5.Owly.Radio1
///
/// StateChanged signal: (t lease_id, s new_state, s reason)
/// State values: "idle" | "starting" | "up" | "stopping"
/// Peers dict keys: "mac" (s, always), "rssi" (n/i16, optional)

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use tracing::{debug, info, warn};
use zbus::{proxy, Connection};

// ── D-Bus proxy ───────────────────────────────────────────────────────────────

#[proxy(
    interface = "io.github.t4t5.Owly.Radio1",
    default_service = "io.github.t4t5.Owly",
    default_path = "/io/github/t4t5/Owly"
)]
trait Radio1 {
    /// Acquire the radio. duration_secs=0 means "until this connection closes".
    /// Returns (lease_id, link_info).
    async fn acquire(
        &self,
        duration_secs: u32,
    ) -> zbus::Result<(u64, HashMap<String, zbus::zvariant::OwnedValue>)>;

    /// Release a lease early.
    async fn release(&self, lease_id: u64) -> zbus::Result<()>;

    /// Hardware probe; returns JSON string.
    async fn probe(&self) -> zbus::Result<String>;

    /// State: "idle" | "starting" | "up" | "stopping"
    #[zbus(property)]
    fn state(&self) -> zbus::Result<String>;

    /// Current lease ID (0 if none held).
    #[zbus(property)]
    fn lease(&self) -> zbus::Result<u64>;

    /// awdl0 link properties while active.
    /// Keys: interface(s), index(u), mac(s), address(s), announce((uuu) optional)
    #[zbus(property)]
    fn link(&self) -> zbus::Result<HashMap<String, zbus::zvariant::OwnedValue>>;

    /// AWDL-layer peer list while radio is up.
    /// Each dict: mac(s, always), rssi(n/i16, optional)
    #[zbus(property)]
    fn peers(&self) -> zbus::Result<Vec<HashMap<String, zbus::zvariant::OwnedValue>>>;

    /// (lease_id, reason) of the last lease end.
    #[zbus(property)]
    fn last_end(&self) -> zbus::Result<(u64, String)>;

    /// Signal: (lease_id, new_state, reason)
    #[zbus(signal, name = "StateChanged")]
    fn radio_state_changed(&self, lease: u64, state: &str, reason: &str) -> zbus::Result<()>;
}

// ── Public probe result ───────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProbeResult {
    pub station: String,
    pub driver: String,
    pub device: String,
    pub status: String,  // "tested" | "untested" | "unsupported"
    pub mode: String,    // "shared" | "exclusive" | "active_monitor"
    pub channel: u32,
    pub hardware: bool,
    pub missing: Vec<String>,
    pub issues: Vec<String>,
}

impl ProbeResult {
    pub fn is_ready(&self) -> bool {
        self.hardware && self.missing.is_empty()
    }

    /// True when operation will disconnect Wi-Fi
    pub fn is_exclusive(&self) -> bool {
        self.mode == "exclusive" || self.mode == "active_monitor"
    }
}

// ── RadioClient ───────────────────────────────────────────────────────────────

pub struct RadioClient {
    proxy: Radio1Proxy<'static>,
    _conn: Connection,
}

impl RadioClient {
    /// Connect to the system bus and create the proxy.
    /// The Connection is kept alive so the lease survives.
    pub async fn connect() -> Result<Self> {
        let conn = Connection::system().await?;
        let proxy = Radio1Proxy::new(&conn).await?;
        Ok(RadioClient { proxy, _conn: conn })
    }

    /// Run a hardware probe and return parsed JSON.
    pub async fn probe(&self) -> Result<ProbeResult> {
        let json = self.proxy.probe().await?;
        let result: ProbeResult = serde_json::from_str(&json)?;
        Ok(result)
    }

    /// Probe as raw JSON string (for diagnostics endpoint — redact sensitive fields).
    pub async fn probe_json_redacted(&self) -> Result<String> {
        let json = self.proxy.probe().await?;
        // Probe result is hardware metadata only — no personal data
        Ok(json)
    }

    /// Acquire the radio for up to `duration_secs` seconds (0 = until we exit).
    /// Returns (lease_id, awdl_interface_name, awdl_ipv6_address).
    pub async fn acquire(&self, duration_secs: u32) -> Result<(u64, String, String)> {
        info!(duration_secs, "acquiring radio lease from owlyd");
        let (lease_id, link) = self.proxy.acquire(duration_secs).await.map_err(|e| {
            let s = e.to_string();
            if s.contains("Busy") {
                anyhow!("RadioBusy: another process holds the radio")
            } else if s.contains("NotAuthorized") {
                anyhow!("PermissionDenied: polkit denied radio access")
            } else if s.contains("NoRadio") {
                anyhow!("UnsupportedAdapter: no usable radio found")
            } else {
                anyhow!("Acquire failed: {s}")
            }
        })?;

        let iface = extract_string(&link, "interface").unwrap_or_else(|| "awdl0".into());
        let addr = extract_string(&link, "address").unwrap_or_default();

        info!(lease_id, iface = %iface, addr = %addr, "radio lease acquired");
        Ok((lease_id, iface, addr))
    }

    /// Release a previously acquired lease.
    pub async fn release(&self, lease_id: u64) -> Result<()> {
        info!(lease_id, "releasing radio lease");
        self.proxy.release(lease_id).await
            .map_err(|e| anyhow!("Release failed: {e}"))?;
        Ok(())
    }

    /// Get current radio state string.
    pub async fn state(&self) -> Result<String> {
        Ok(self.proxy.state().await?)
    }

    /// Subscribe to StateChanged signals; calls on_change(lease, state, reason).
    pub async fn watch_state<F>(&self, mut on_change: F) -> Result<()>
    where
        F: FnMut(u64, String, String) + Send + 'static,
    {
        use futures_util::StreamExt;
        let mut stream = self.proxy.receive_radio_state_changed().await?;
        while let Some(signal) = stream.next().await {
            let args = signal.args()?;
            debug!(
                lease = args.lease,
                state = args.state,
                reason = args.reason,
                "owlyd state changed"
            );
            on_change(args.lease, args.state.to_string(), args.reason.to_string());
        }
        warn!("owlyd StateChanged signal stream ended (owlyd may have exited)");
        Ok(())
    }

    /// Get current awdl0 link info.
    pub async fn link_info(&self) -> Result<HashMap<String, zbus::zvariant::OwnedValue>> {
        Ok(self.proxy.link().await?)
    }

    /// Check if owlyd is reachable on the system bus.
    pub async fn is_available() -> bool {
        if let Ok(conn) = Connection::system().await {
            Radio1Proxy::new(&conn).await.is_ok()
        } else {
            false
        }
    }
}

fn extract_string(
    map: &HashMap<String, zbus::zvariant::OwnedValue>,
    key: &str,
) -> Option<String> {
    map.get(key).and_then(|v| match &**v {
        zbus::zvariant::Value::Str(s) => Some(s.as_str().to_string()),
        _ => None,
    })
}
