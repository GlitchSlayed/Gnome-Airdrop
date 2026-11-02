# ADR-001: Architecture Decision — Backend Reuse vs Bridge vs Fork

**Date:** 2026-11-02  
**Status:** Accepted  
**Owly version pinned:** 0.4.2 (commit built from t4t5/owly tag v0.4.2)

---

## Context

We need a GNOME Shell extension that provides native AirDrop functionality via
Owly. Owly 0.4.2 ships two components:

1. **`owlyd`** — a system service (runs as root with CAP_NET_ADMIN + CAP_NET_RAW).
   Exposes `io.github.t4t5.Owly.Radio1` on the **system D-Bus**.
   Manages the Wi-Fi radio, creates `awdl0`, provides AWDL peer discovery.

2. **`owly` CLI** — an unprivileged user-space binary.
   Acquires a radio lease from `owlyd`, then runs mDNS discovery and AirDrop
   HTTP transfers entirely in user space over `awdl0`.

### Verified Radio1 D-Bus API (system bus)

```
Object: /io/github/t4t5/Owly
Interface: io.github.t4t5.Owly.Radio1

Methods:
  Acquire(u duration_secs) -> (t lease_id, a{sv} link_info)
    duration_secs = 0 means "until caller exits"
    link_info keys: interface(s), index(u), address(s), mac(s), announce(uuu)
    Radio is RELEASED automatically when the calling process exits.
    
  AcquireDetached(u duration_secs) -> (t lease_id, a{sv} link_info)
    Only root may call this (lease survives caller exit).
    
  Release(t lease_id) -> ()
  
  Probe() -> s  (JSON string with hardware diagnostics)

Properties (all emit-change):
  State(s)         "idle" | "acquiring" | "active" | "stopping" | "error"
  Lease(t)         current lease_id, 0 if none
  Peers(aa{sv})    AWDL-layer peers while active; key: "mac"(s), "name"(s)?
  Link(a{sv})      current awdl0 link info (same as Acquire return)
  LastEnd((ts))    (lease_id, reason_string) of the last lease end

Signals:
  StateChanged(t lease_id, s old_state, s new_state)
  PropertiesChanged (standard)
```

### What owlyd does NOT provide

- mDNS browse/announce over awdl0
- AirDrop HTTP discovery (/Discover endpoint, port 8770)
- AirDrop HTTP transfer (HTTPS server, port 8771 default)
- Accept/reject UI
- File I/O

These happen entirely in the `owly` CLI user-space code.

### Hardware context

- Intel AX200 (8086:2723, iwlwifi), **exclusive mode**, channel 6.
- Radio acquisition disconnects Wi-Fi on wlo1. Interface names detected dynamically.
- `owly doctor` confirms: `status=tested, mode=exclusive`.

---

## Decision

### We do NOT fork Owly.

Owly provides all radio management we need. No upstream changes are required.

### We do NOT build a full replacement for owly CLI.

That would re-implement the AirDrop protocol unnecessarily.

### We DO build a narrow session bridge: `airdrop-bridge`

The bridge is a small Rust binary that:
1. **Holds a radio lease** from `owlyd` (Acquire → awdl0 interface comes up)
2. **Wraps `owly` subprocesses** for browse, send, receive (with structured stdout parsing via `--json` flag when available, or purpose-built output)
3. **Exposes a versioned session D-Bus interface** (`io.github.t4t5.AirDropGnome1`) on the **session bus**
4. **Manages state machines** for radio, visibility, and transfers
5. **Handles incoming approval** (sends D-Bus signals for the extension to present Accept/Reject UI)

The bridge does NOT run inside GNOME Shell. It is a separate user process.

### The extension

- TypeScript → compiled GJS
- GNOME Shell 50 Quick Settings (SystemIndicator + QuickMenuToggle)
- Communicates only with `airdrop-bridge` over session D-Bus
- No network, file I/O, or synchronous work on the Shell thread

---

## Session D-Bus Interface Design

```
Bus: session
Service: io.github.t4t5.AirDropGnome
Object: /io/github/t4t5/AirDropGnome

Interface: io.github.t4t5.AirDropGnome1
  
  Methods:
    GetVersion() -> (u api_version, s bridge_version, a{sv} capabilities)
    GetRadioState() -> a{sv}   (full state snapshot)
    
    SetReceiving(s mode, u duration_secs) -> ()
      mode: "off" | "timed" | "always"
      duration_secs: used when mode="timed"
    
    Browse() -> ()              start/refresh discovery
    StopBrowse() -> ()
    
    SendFiles(as files, s peer_id) -> u transfer_id
    SendUrl(s url, s peer_id) -> u transfer_id
    CancelTransfer(u transfer_id) -> ()
    
    AcceptIncoming(u offer_id) -> ()
    RejectIncoming(u offer_id) -> ()
    
    GetTransfers() -> aa{sv}   (all active transfers)
    
    OpenReceivedDir() -> ()
    GetDiagnostics() -> s      (redacted JSON)

  Signals:
    RadioStateChanged(a{sv} state)    (radio/visibility state)
    PeerAdded(a{sv} peer)
    PeerRemoved(s peer_id)
    PeerChanged(a{sv} peer)
    IncomingOffer(a{sv} offer)        (for Accept/Reject UI)
    TransferProgress(a{sv} transfer)
    TransferCompleted(a{sv} transfer)
    TransferFailed(a{sv} transfer)
    TransferCancelled(u transfer_id)

  Properties:
    ApiVersion(u)              = 1
    RadioState(s)              mirrors owlyd state + our visibility
    ReceivingMode(s)           "off" | "timed" | "always"
    ReceivingExpiry(t)         Unix timestamp, 0 if not timed
    BackendAvailable(b)        owlyd reachable and API version ok
    AdapterReady(b)            hardware probe passed
```

---

## Responsibility Matrix

| Concern | owlyd | airdrop-bridge | extension |
|---|---|---|---|
| Radio acquisition | ✅ | calls Acquire | notified |
| AWDL peer layer | ✅ | monitors Peers | — |
| Consent before exclusive mode | — | prompts via D-Bus signal | shows dialog |
| mDNS discovery | — | runs owly browse | shows peer list |
| AirDrop HTTP send | — | runs owly send | triggers |
| AirDrop HTTP receive | — | runs owly receive | shows incoming |
| Incoming approval | — | waits for AcceptIncoming | shows dialog |
| File I/O | — | owly CLI | — |
| Progress events | — | parses owly stdout | shows bar |
| State persistence | — | preferences file | reads prefs |
| UI | — | — | ✅ |

---

## Deferred

- Direct Rust AirDrop protocol implementation (not needed: owly CLI covers it)
- Clipboard sharing (M3)
- Copyous integration (M3)
- Nautilus integration (M3)
- RPM/DEB packaging (M4)
