/// owly_process.rs — subprocess wrappers for owly browse, send, receive

use anyhow::{anyhow, Result};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::state::{IncomingOffer, Peer};

// ── Browse ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum BrowseEvent {
    PeerFound(Peer),
    Done,
    Error(String),
}

/// Run `owly browse --seconds N` and emit peers via the channel.
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
    let stderr = child.stderr.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let mut err_lines = BufReader::new(stderr).lines();

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
                        debug!(line = %l, "owly browse stdout");
                        if let Some(peer) = parse_browse_line(&l) {
                            let _ = tx.send(BrowseEvent::PeerFound(peer)).await;
                        }
                    }
                    Ok(None) => {
                        let _ = tx.send(BrowseEvent::Done).await;
                        break;
                    }
                    Err(e) => {
                        let _ = tx.send(BrowseEvent::Error(e.to_string())).await;
                        break;
                    }
                }
            }
            err_line = err_lines.next_line() => {
                if let Ok(Some(l)) = err_line {
                    debug!(line = %l, "owly browse stderr");
                    if let Some(peer) = parse_browse_line(&l) {
                        let _ = tx.send(BrowseEvent::PeerFound(peer)).await;
                    }
                }
            }
        }
    }

    let _ = child.wait().await;
    Ok(())
}

/// Parse a single line from `owly browse` output.
/// owly browse format:
///   <instance_id>  "<name>"  (<addr>)
/// or with found:
///   found  <name>  <service_id>  <mac>
fn parse_browse_line(line: &str) -> Option<Peer> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Format 1: 12-char hex instance, then "Name", then (addr)
    // Example: "3fa9c01b22de   \"Alice's iPhone\" (fe80::...)"
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    if let Some(first_quote) = line.find('"') {
        if let Some(second_quote) = line[first_quote + 1..].find('"') {
            let name = &line[first_quote + 1..first_quote + 1 + second_quote];
            let instance = tokens[0].trim().to_string();
            let addr = line.rfind('(')
                .and_then(|open| line.rfind(')').map(|close| &line[open + 1..close]))
                .unwrap_or("")
                .to_string();

            let device_type = device_type_hint(name);
            return Some(Peer {
                id: instance,
                name: name.to_string(),
                mac: addr,
                device_type,
                last_seen: Some(std::time::Instant::now()),
            });
        }
    }

    // Format 2: "found  Name  service_id  mac"
    if line.starts_with("found") {
        let parts: Vec<&str> = line.splitn(4, char::is_whitespace)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() >= 3 {
            let name = parts.get(1).copied().unwrap_or("Unknown").to_string();
            let service_id = parts.get(2).copied().unwrap_or("").to_string();
            let mac = parts.get(3).copied().unwrap_or("").to_string();
            let device_type = device_type_hint(&name);
            return Some(Peer {
                id: service_id,
                name,
                mac,
                device_type,
                last_seen: Some(std::time::Instant::now()),
            });
        }
    }

    None
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
    Progress(u64, u64),
    Done,
    Rejected,
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
    let mut err_lines = BufReader::new(stderr).lines();

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
                        if let Some(ev) = parse_send_line(&l) {
                            let is_terminal = matches!(ev, SendEvent::Done | SendEvent::Rejected | SendEvent::Error(_));
                            let _ = tx.send(ev).await;
                            if is_terminal { break; }
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let _ = tx.send(SendEvent::Error(e.to_string())).await;
                        break;
                    }
                }
            }
            err_line = err_lines.next_line() => {
                if let Ok(Some(l)) = err_line {
                    debug!(line = %l, "owly send stderr");
                    if let Some(ev) = parse_send_line(&l) {
                        let is_terminal = matches!(ev, SendEvent::Done | SendEvent::Rejected | SendEvent::Error(_));
                        let _ = tx.send(ev).await;
                        if is_terminal { break; }
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

fn parse_send_line(line: &str) -> Option<SendEvent> {
    let line = line.trim();
    if line == "done" || line == "delivered" {
        return Some(SendEvent::Done);
    }
    if line == "rejected" || line == "declined" {
        return Some(SendEvent::Rejected);
    }
    if let Some(rest) = line.strip_prefix("failed:") {
        return Some(SendEvent::Error(rest.trim().to_string()));
    }
    if let Some(rest) = line.strip_prefix("error:") {
        return Some(SendEvent::Error(rest.trim().to_string()));
    }
    if line.contains("bytes") && line.contains('/') {
        // "12345 / 100000 bytes"
        let parts: Vec<&str> = line.split('/').collect();
        if parts.len() == 2 {
            let sent = parts[0].trim().parse::<u64>().ok()?;
            let total_part = parts[1].trim().trim_end_matches("bytes").trim();
            let total = total_part.parse::<u64>().ok()?;
            return Some(SendEvent::Progress(sent, total));
        }
    }
    None
}

// ── Receive ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ReceiveEvent {
    Offer(IncomingOffer),
    Accepted(u32, String),
    Progress(u32, u64, u64),
    Done(u32),
    Error(String),
}

/// Start `owly receive --dir <dir>`
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
    let stderr = child.stderr.take().unwrap();
    let stdin = child.stdin.take().unwrap();
    let (ev_tx, ev_rx) = mpsc::channel::<ReceiveEvent>(32);
    let (accept_tx, mut accept_rx) = mpsc::channel::<bool>(8);

    // Stdin writer for Accept/Reject
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let mut stdin = stdin;
        while let Some(accepted) = accept_rx.recv().await {
            let response = if accepted { b"y\n" as &[u8] } else { b"n\n" };
            let _ = stdin.write_all(response).await;
        }
    });

    // Stdout & Stderr reader
    let ev_tx2 = ev_tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut err_lines = BufReader::new(stderr).lines();
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
                err_line = err_lines.next_line() => {
                    match err_line {
                        Ok(Some(l)) => {
                            debug!(line = %l, "owly receive stderr");
                            if let Some(ev) = parse_receive_line(&l, &mut offer_counter) {
                                let _ = ev_tx2.send(ev).await;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
        }
        let _ = child.wait().await;
    });

    Ok((ev_rx, accept_tx))
}

fn parse_receive_line(line: &str, offer_counter: &mut u32) -> Option<ReceiveEvent> {
    let line = line.trim();

    // Check for "wants to send:"
    if line.contains("wants to send:") {
        *offer_counter += 1;
        let id = *offer_counter;
        let sender_name = if let Some(first_quote) = line.find('"') {
            if let Some(second_quote) = line[first_quote + 1..].find('"') {
                line[first_quote + 1..first_quote + 1 + second_quote].to_string()
            } else {
                "Nearby Device".to_string()
            }
        } else {
            "Nearby Device".to_string()
        };

        return Some(ReceiveEvent::Offer(IncomingOffer {
            offer_id: id,
            sender_name,
            file_names: vec!["File transfer".into()],
            total_bytes: 0,
        }));
    }

    if let Some(rest) = line.strip_prefix("transfer failed:") {
        return Some(ReceiveEvent::Error(rest.trim().to_string()));
    }

    None
}
