/// owly_process.rs — subprocess wrappers for owly browse, send, receive
///
/// These functions spawn the `owly` CLI and parse its structured output.
/// We do NOT parse localized human-readable prose; instead we rely on:
///   - owly browse: JSON-line output per discovered peer (--json flag if available,
///     else line-by-line structured parsing of the known stable output format)
///   - owly send: progress lines "progress: N/TOTAL" and "done" / "rejected" / "error: ..."
///   - owly receive: "offer: NAME FILES BYTES", "accepted", "done" etc.
///
/// All subprocesses are bounded (timeouts, cancellation tokens).

use anyhow::{anyhow, Result};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::state::{IncomingOffer, Peer};

// ── Browse ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum BrowseEvent {
    /// A peer appeared or was refreshed
    PeerFound(Peer),
    /// Browse completed (timeout reached)
    Done,
    /// Browse failed
    Error(String),
}

/// Run `owly browse --seconds N` and emit peers via the channel.
/// The awdl_iface is the interface name (e.g. "awdl0").
pub async fn browse(
    awdl_iface: &str,
    seconds: u32,
    display_name: Option<&str>,
    cancel: CancellationToken,
    tx: mpsc::Sender<BrowseEvent>,
) -> Result<()> {
    let mut cmd = Command::new("owly");
    cmd.arg("browse")
        .arg("--iface").arg(awdl_iface)
        .arg("--seconds").arg(seconds.to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    if let Some(name) = display_name {
        cmd.arg("--name").arg(name);
    }

    info!(iface = awdl_iface, seconds, "starting owly browse");
    let mut child = cmd.spawn().map_err(|e| anyhow!("owly browse spawn failed: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("browse cancelled");
                let _ = child.kill().await;
                break;
            }
            line = lines.next_line() => {
                match line {
                    Ok(Some(l)) => {
                        debug!(line = %l, "owly browse output");
                        if let Some(peer) = parse_browse_line(&l) {
                            let _ = tx.send(BrowseEvent::PeerFound(peer)).await;
                        }
                    }
                    Ok(None) => {
                        // EOF
                        let _ = tx.send(BrowseEvent::Done).await;
                        break;
                    }
                    Err(e) => {
                        let _ = tx.send(BrowseEvent::Error(e.to_string())).await;
                        break;
                    }
                }
            }
        }
    }

    let _ = child.wait().await;
    Ok(())
}

/// Parse a single line from `owly browse` output.
/// owly browse output format (stable as of 0.4.2):
///   found  <name>  <service_id>  <mac>
/// e.g.:
///   found  iPhone  _airdrop._tcp/fa6b5071-88d9  fe80::f86b:50ff:fe71:88d9%awdl0
fn parse_browse_line(line: &str) -> Option<Peer> {
    let line = line.trim();
    if !line.starts_with("found") {
        return None;
    }

    // Split on whitespace runs
    let parts: Vec<&str> = line.splitn(4, char::is_whitespace)
        .filter(|s| !s.is_empty())
        .collect();

    // parts[0] = "found", parts[1] = name, parts[2] = service_id, parts[3] = address
    if parts.len() < 3 {
        return None;
    }

    let name = parts.get(1).copied().unwrap_or("Unknown").to_string();
    let service_id = parts.get(2).copied().unwrap_or("").to_string();
    let mac = parts.get(3).copied().unwrap_or("").to_string();

    // Derive device type from name heuristics (not trusted for security)
    let device_type = device_type_hint(&name);

    Some(Peer {
        id: service_id,
        name,
        mac,
        device_type,
        last_seen: Some(std::time::Instant::now()),
    })
}

fn device_type_hint(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.contains("iphone") || lower.contains("phone") {
        "phone".into()
    } else if lower.contains("ipad") || lower.contains("tablet") {
        "tablet".into()
    } else if lower.contains("mac") || lower.contains("book") || lower.contains("laptop") {
        "laptop".into()
    } else {
        "unknown".into()
    }
}

// ── Send ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum SendEvent {
    /// Progress update: (transferred_bytes, total_bytes)
    Progress(u64, u64),
    /// Transfer completed successfully
    Done,
    /// Remote rejected the transfer
    Rejected,
    /// An error occurred
    Error(String),
}

/// Run `owly send --to <peer_id> [files...]` and emit events.
pub async fn send_files(
    awdl_iface: &str,
    files: &[String],
    peer_id: &str,
    display_name: Option<&str>,
    discovery_secs: u32,
    cancel: CancellationToken,
    tx: mpsc::Sender<SendEvent>,
) -> Result<()> {
    let mut cmd = Command::new("owly");
    cmd.arg("send")
        .arg("--iface").arg(awdl_iface)
        .arg("--to").arg(peer_id)
        .arg("--seconds").arg(discovery_secs.to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    if let Some(name) = display_name {
        cmd.arg("--name").arg(name);
    }

    for f in files {
        cmd.arg(f);
    }

    info!(peer_id, files = ?files, "starting owly send");
    let mut child = cmd.spawn().map_err(|e| anyhow!("owly send spawn failed: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let _err_lines = BufReader::new(stderr).lines();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!("send cancelled");
                let _ = child.kill().await;
                let _ = tx.send(SendEvent::Error("cancelled".into())).await;
                break;
            }
            line = lines.next_line() => {
                match line {
                    Ok(Some(l)) => {
                        debug!(line = %l, "owly send stdout");
                        match parse_send_line(&l) {
                            Some(ev) => {
                                let is_terminal = matches!(ev, SendEvent::Done | SendEvent::Rejected | SendEvent::Error(_));
                                let _ = tx.send(ev).await;
                                if is_terminal { break; }
                            }
                            None => {}
                        }
                    }
                    Ok(None) => {
                        // EOF without explicit done/error - check exit status
                        break;
                    }
                    Err(e) => {
                        let _ = tx.send(SendEvent::Error(e.to_string())).await;
                        break;
                    }
                }
            }
        }
    }

    let status = child.wait().await?;
    if !status.success() && tx.capacity() > 0 {
        let _ = tx.send(SendEvent::Error(format!("owly send exited with {status}"))).await;
    }
    Ok(())
}

/// Parse send progress/result lines.
/// owly send output (0.4.2):
///   "progress: 12345 / 100000"  (bytes)
///   "done"
///   "rejected"
///   "error: <message>"
fn parse_send_line(line: &str) -> Option<SendEvent> {
    let line = line.trim();
    if line == "done" {
        return Some(SendEvent::Done);
    }
    if line == "rejected" {
        return Some(SendEvent::Rejected);
    }
    if let Some(rest) = line.strip_prefix("error:") {
        return Some(SendEvent::Error(rest.trim().to_string()));
    }
    if let Some(rest) = line.strip_prefix("progress:") {
        // "12345 / 100000"
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 {
            let sent = parts[0].trim().parse::<u64>().ok()?;
            let total = parts[1].trim().parse::<u64>().ok()?;
            return Some(SendEvent::Progress(sent, total));
        }
    }
    None
}

// ── Receive ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ReceiveEvent {
    /// An incoming offer arrived
    Offer(IncomingOffer),
    /// An offer was accepted (offer_id, dest_path)
    Accepted(u32, String),
    /// Progress on an accepted transfer
    Progress(u32, u64, u64),
    /// Transfer completed
    Done(u32),
    /// Error
    Error(String),
}

/// Handle for the running receive process, with a channel to accept/reject offers.
pub struct ReceiveHandle {
    pub child: Child,
    pub events: mpsc::Receiver<ReceiveEvent>,
    pub next_offer_id: u32,
}

/// Start `owly receive --dir <dir>` and return a handle.
/// The process runs until cancelled.
/// IMPORTANT: owly receive with `--accept` accepts everything automatically.
/// We do NOT pass --accept; instead we parse "offer:" lines and wait for
/// our AcceptIncoming D-Bus call before writing to disk (controlled via stdin).
///
/// Note: owly 0.4.2 receive reads 'y'/'n' on stdin per offer.
pub async fn start_receive(
    awdl_iface: &str,
    dest_dir: &Path,
    port: u16,
    display_name: Option<&str>,
    cancel: CancellationToken,
) -> Result<(mpsc::Receiver<ReceiveEvent>, tokio::sync::mpsc::Sender<bool>)> {
    let mut cmd = Command::new("owly");
    cmd.arg("receive")
        .arg("--iface").arg(awdl_iface)
        .arg("--dir").arg(dest_dir)
        .arg("--port").arg(port.to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    if let Some(name) = display_name {
        cmd.arg("--name").arg(name);
    }

    info!(iface = awdl_iface, dir = %dest_dir.display(), port, "starting owly receive");
    let mut child = cmd.spawn().map_err(|e| anyhow!("owly receive spawn failed: {e}"))?;

    let stdout = child.stdout.take().unwrap();
    let stdin = child.stdin.take().unwrap();
    let (ev_tx, ev_rx) = mpsc::channel::<ReceiveEvent>(32);
    let (accept_tx, mut accept_rx) = mpsc::channel::<bool>(8);

    // Spawn stdin writer task (responds to Accept/Reject signals)
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let mut stdin = stdin;
        while let Some(accepted) = accept_rx.recv().await {
            let response = if accepted { b"y\n" as &[u8] } else { b"n\n" };
            let _ = stdin.write_all(response).await;
        }
    });

    // Spawn stdout reader task
    let ev_tx2 = ev_tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut offer_counter = 0u32;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("receive process cancelled");
                    let _ = child.kill().await;
                    break;
                }
                line = lines.next_line() => {
                    match line {
                        Ok(Some(l)) => {
                            debug!(line = %l, "owly receive stdout");
                            if let Some(ev) = parse_receive_line(&l, &mut offer_counter) {
                                let _ = ev_tx2.send(ev).await;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            let _ = ev_tx2.send(ReceiveEvent::Error(e.to_string())).await;
                            break;
                        }
                    }
                }
            }
        }
        let _ = child.wait().await;
    });

    Ok((ev_rx, accept_tx))
}

/// Parse receive output lines.
/// owly receive output (0.4.2):
///   "offer: <sender_name> [file1, file2, ...] <total_bytes>"
///   "accepted"  (after we write 'y' to stdin)
///   "progress: <offer_id> <sent> / <total>"
///   "done: <offer_id> <dest_path>"
///   "error: <message>"
fn parse_receive_line(line: &str, offer_counter: &mut u32) -> Option<ReceiveEvent> {
    let line = line.trim();

    if let Some(rest) = line.strip_prefix("offer:") {
        *offer_counter += 1;
        let id = *offer_counter;
        // Parse: "SenderName file1.jpg,file2.png 12345"
        // Split from end: last token = bytes, second-to-last = files, rest = name
        let parts: Vec<&str> = rest.trim().rsplitn(3, ' ').collect();
        let total_bytes = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0u64);
        let file_list = parts.get(1).copied().unwrap_or("").to_string();
        let sender_name = parts.get(2).copied().unwrap_or("Unknown").trim().to_string();
        let file_names: Vec<String> = file_list.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        return Some(ReceiveEvent::Offer(IncomingOffer {
            offer_id: id,
            sender_name,
            file_names,
            total_bytes,
        }));
    }

    if let Some(rest) = line.strip_prefix("done:") {
        let parts: Vec<&str> = rest.trim().splitn(2, ' ').collect();
        let offer_id = parts.first().and_then(|s| s.parse().ok()).unwrap_or(*offer_counter);
        let dest = parts.get(1).copied().unwrap_or("").to_string();
        return Some(ReceiveEvent::Done(offer_id));
    }

    if let Some(rest) = line.strip_prefix("progress:") {
        // "offer_id sent / total"
        let parts: Vec<&str> = rest.trim().splitn(3, ' ').collect();
        let offer_id = parts.first().and_then(|s| s.parse().ok()).unwrap_or(*offer_counter);
        if let (Some(sent_str), Some(total_str)) = (parts.get(1), parts.get(2)) {
            let sent = sent_str.trim_end_matches('/').trim().parse().unwrap_or(0u64);
            let total = total_str.trim_start_matches('/').trim().parse().unwrap_or(0u64);
            return Some(ReceiveEvent::Progress(offer_id, sent, total));
        }
    }

    if let Some(rest) = line.strip_prefix("error:") {
        return Some(ReceiveEvent::Error(rest.trim().to_string()));
    }

    None
}
