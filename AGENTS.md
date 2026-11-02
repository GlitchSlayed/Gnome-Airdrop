# AGENTS.md — AirDrop for GNOME using Owly

## Mission and scope

Build `airdrop-gnome`: a polished GNOME Shell extension that brings real AirDrop sharing into Quick Settings on Fedora and Ubuntu, powered by Owly: https://github.com/t4t5/owly.

The intended experience is: install Owly and the extension, open a native AirDrop menu, see nearby discoverable recipients, select a device, and send files or clipboard content. Receiving must support Off, On for 10 minutes, and Always on, with explicit approval for incoming transfers. Include progress, cancellation, notifications, useful diagnostics, and optional Files and Copyous integration.

Implement this project; do not stop at another roadmap or a visual mockup. Work in incremental, testable milestones. If hardware or permissions prevent a test, continue independent work and clearly record what remains unverified. Do not claim real-device success from mocks or from `owly doctor` alone.

Keep the product centered on the extension. A small background bridge and native dialogs are appropriate when necessary; a large standalone application is not the main deliverable. Avoid accounts, cloud services, telemetry, remote relays, and unrelated cross-platform sharing protocols.

## Existing repository and working rules

- Inspect the repository, existing instructions, build system, and git status before changing code. Adapt this layout to existing conventions and preserve unrelated changes.
- Make routine implementation decisions autonomously within this scope. Do not repeatedly ask the user to choose internal libraries, filenames, or UI details.
- Keep networking, archive processing, and long-running work outside GNOME Shell. All Shell-facing work must be asynchronous and bounded.
- Do not install privileged components, disconnect the host's Wi-Fi, change firewall rules, publish releases, or contact upstream maintainers without authorization for that action. Prepare packages, patches, documentation, and tests first. Ordinary local implementation does not need repeated confirmation.
- Do not silently download and execute privileged installation scripts from the extension.
- Maintain a concise implementation checklist and evidence log. Clearly distinguish implemented, mocked, hardware-tested, blocked, and deferred features.

## Known user hardware

The user supplied this diagnostic output:

```text
station   wlo1 (iwlwifi, 8086:2723), tested
mode      exclusive, channel 6
card      phy0: ad-hoc yes, AP yes, active monitor no, monitor beside the station yes
regdom    US (the driver's own)
channels  6 ok, 44 ok, 149 ok
ready
note: Intel cards run on channel 6: no phone has used their ad-hoc cell on 5 GHz.
note: AirDrop runs on channel 6 here, where it gets one or two of the iPhone's sixteen slots, so transfers are slow: a 2.4 MB photo takes 10-17 s.
```

Treat this as the initial real-world target, not a universal adapter configuration:

- Owly recognizes this adapter as tested and ready; successful transfers still need verification.
- Exclusive mode takes this adapter away from normal Wi-Fi while the radio is active.
- A monitor interface existing beside a station does not prove that working shared-mode AirDrop is available.
- Do not force shared mode, force 5 GHz, or override regulatory restrictions to conceal the limitation.
- Always-on receiving can keep this adapter unavailable for ordinary Wi-Fi. Explain that before activation. Ethernet or a separately selected compatible adapter may maintain normal connectivity.
- The diagnostic speed example is not a performance guarantee. Support slow progress without premature timeout assumptions.
- Detect interface names and hardware dynamically. Never hardcode `wlo1`, `phy0`, PCI IDs, channels, or the country into operational code.

## Upstream strategy and feasibility audit

Prefer upstream Owly. Inspect and pin an actual commit or release before integrating. README claims, protocol specifications, proposed APIs in this document, and implemented APIs are different evidence levels.

Audit the code for:

1. Privileged radio acquisition, ownership, release, crash recovery, and authorization.
2. Discovery events, BLE wake-up behavior, peer identity, duplicate names, and expiration.
3. Send/receive APIs, incoming-offer accept/reject, and approval deadlines.
4. Progress, cancellation, errors, file batches, archive handling, and link transfers.
5. Concurrent clients and coordination with the CLI or Omdrop.
6. Available machine-readable IPC and the feasibility of direct Rust library reuse.
7. Existing packaging, service units, firewall handling, licenses, and test coverage.

Reuse existing capabilities instead of recreating AWDL or AirDrop. A CLI wrapper may prove feasibility, but do not build production transfer state by scraping localized human-readable output. If upstream lacks necessary hooks, implement a narrow adapter or prepare small upstreamable changes. Maintain a fork only when required, document each divergence, and avoid unnecessary protocol rewrites. Publishing a fork or submitting changes is a separate action from preparing them.

Do not promise Contacts Only, Apple account verification, Universal Clipboard, NameDrop, Android support, or automatic iOS clipboard insertion. These are outside initial scope. Verify any broader interoperability before advertising it.

## Architecture

Preferred structure:

- **GNOME Shell extension:** Quick Settings tile and menu, recipient list, lightweight transfer UI, notifications, explicit clipboard capture, preferences entry point.
- **Owly desktop API:** reuse a suitable upstream API if present. Otherwise add a small user-session service using Owly libraries and exposing a versioned D-Bus interface.
- **Owly privileged service:** retain upstream radio management and authorization boundaries; never run Shell, clipboard handling, or file extraction as root.
- **Native auxiliary dialogs:** file chooser, payload preview, device selection where necessary, setup and diagnostics. Prefer GTK4/libadwaita; use extension preferences for settings where practical.
- **Optional integrations:** Nautilus context menu and Copyous actions, separately installable and gracefully absent.

Prefer TypeScript compiled to readable GJS for the extension and Rust for a new bridge. Node.js is build tooling only, not a required Shell runtime. Use project conventions when equivalent existing infrastructure is available.

Keep upstream-specific code behind one backend adapter. A mock backend must exercise the same contract as the real backend. Do not build a general transport plugin framework before the Owly path works.

Suggested directories, adjustable to the repository:

```text
extension/          Shell UI, preferences, schemas, translations
service/            Optional Rust bridge and Owly adapter
dialogs/            Small auxiliary UI if required
integrations/       Nautilus and Copyous integration
packaging/          RPM, DEB, service units, policies
tests/              API, lifecycle, payload, UI and integration tests
docs/               Setup, architecture, compatibility and hardware results
```

## Quick Settings and interaction design

Use GNOME's SystemIndicator and QuickMenuToggle patterns. Match GNOME typography, spacing, symbolic icons, focus behavior, accessibility, light/dark themes, and high-contrast behavior. Avoid a webview or oversized custom desktop inside the menu.

The tile controls receiving visibility. It must reflect confirmed backend state, not merely the last requested setting. Represent Off, Starting, Receiving with remaining time, Transferring, Restoring Wi-Fi, and Setup required/error without falsely suggesting success. Keep actual radio state, visibility, and transfer activity independently modeled.

Menu content:

1. Receiving selector: **Off**, **On for 10 minutes**, **Always on**.
2. Nearby discoverable AirDrop recipients with device names, available device-type icons, scanning state, empty state, and refresh.
3. Send files and Send clipboard actions. Selecting a recipient can open payload actions; selecting payloads first can open the recipient chooser.
4. Active transfers with progress, waiting/rejected/failed states, cancel and retry.
5. Choose from clipboard history when Copyous is detected.
6. Open received files, Settings, and Diagnostics.

Receiving semantics:

- Default to Off until first explicit activation; normal quick activation uses a 10-minute window.
- Always on means continuously discoverable for receiving, not automatic acceptance. Require approval per transfer.
- Visibility expiry stops new offers while accepted transfers finish. Off stops new offers; show an explicit Cancel transfers and stop action for immediate shutdown.
- Persist the preferred receiving mode separately from current radio state. Restore always-on after login only with an explicit startup preference and valid adapter/mode consent. Never revive a timed session or disruptive radio state merely because a process restarted.
- Before exclusive operation, explain that normal Wi-Fi on the selected adapter will disconnect. Any remembered consent is scoped to adapter and operating mode; invalidate it when these change.
- When idle receiving still owns the radio, do not label it as radio Off.

Discovery semantics:

- List discoverable AirDrop recipients, not all nearby Bluetooth devices or access points.
- Browse while the menu/device picker needs results; stop unnecessary outbound browsing when closed. Keep any operations required for explicitly enabled receiving active.
- If browsing itself requires exclusive radio ownership, obtain the same consent before starting it. Simply opening a menu must not silently disconnect Wi-Fi.
- Expire stale peers and distinguish duplicate names. Use opaque backend identities, not display names, to address transfers. Do not treat device names as authenticated identities.
- Show actionable guidance when Apple visibility settings or missing Bluetooth/radio prerequisites prevent discovery, based on the verified backend behavior.

## File transfer workflows

Sending: select/capture payload, validate access and limits, explain radio impact, acquire the radio, discover/select a recipient, await recipient approval, transfer, finalize, and release ownership when no operation needs it.

Receiving: enable visibility, present the incoming sender label and payload summary, accept/reject within the actual protocol deadline, write to private staging, finalize successfully received content into the configured destination, and provide completion actions.

- Use a native file chooser and support multiple files.
- Do not make outgoing sharing depend on general receive visibility being enabled.
- Show indeterminate progress when totals are unavailable; never invent percentages.
- Cancellation and retry must not silently duplicate transfers or overwrite files.
- Never automatically open incoming files or URLs. Offer Open folder and explicit suitable Copy actions.
- Use the user's configured download directory, respecting XDG paths and permissions. Resolve collisions without overwriting.
- Treat folder sharing as a later capability until protocol and extraction behavior are tested.

## Clipboard and Copyous

Core clipboard sharing must work without Copyous or any other clipboard manager. Support Wayland through appropriate GNOME APIs; do not depend on X11 tools or global clipboard scraping.

| Payload | Initial behavior |
| --- | --- |
| Plain text and code | Preserve UTF-8 and newlines; send a text file |
| HTTP/HTTPS URL | Native link sharing only if implemented and verified; offer text-file fallback |
| Images | Bounded PNG capture/encoding after text support works |
| Copied local files | Validate and send referenced files, not the literal path list |
| Rich text | Explicit plain-text fallback initially |
| Unsupported or empty content | Clear explanation; do not send bogus data |

Only read and transmit clipboard content after explicit interaction. Snapshot the chosen content so later clipboard changes cannot change a queued transfer. Show a bounded payload preview and recipient confirmation within the flow. No continuous synchronization, background history export, automatic forwarding, or secret/OTP heuristics presented as guaranteed protection.

Keep clipboard payloads out of command-line arguments, logs, diagnostics and persistent history. Use bounded streams or file descriptors for large data. Keep temporary content private and clean it up on completion, cancellation, failure and restart.

Declare clipboard access in extension metadata and follow current GNOME review rules. Clipboard shortcuts must be unassigned by default; allow users to configure them.

Copyous integration is optional and incremental:

1. Detect its documented D-Bus service and use Show to open history; the user copies an item and then shares the current clipboard.
2. Inspect the current customizable-action interface for a direct Send via AirDrop action on a selected item.
3. If safe typed item export is unavailable, prepare an upstream API proposal/patch and retain the basic workflow.

Previously documented Copyous service: `org.gnome.Shell.Extensions.Copyous`, object `/org/gnome/Shell/Extensions/Copyous`. Revalidate current introspection before use. Documented dialog/history-management methods do not imply a selected-item export API.

Do not scrape its database, monkey-patch internals, copy its source without license review, or clear its history. Do not interpolate arbitrary clipboard contents into shell commands. Disable integration cleanly when Copyous disappears or is incompatible.

## API, lifecycle and recovery

The following are required capabilities, not claims about existing Owly methods. Define a versioned contract using actual upstream capabilities:

- API/backend versions and capability flags.
- Hardware diagnostics, adapter selection and operating-mode impact.
- Start/stop receiving with a bounded duration or explicit persistent mode.
- Start/stop discovery, peer added/changed/removed events.
- Asynchronous send, accept, reject, cancel and explicit retry.
- Transfer enumeration and progress/state/error events.
- Radio ownership and networking restoration status.

Return request or transfer handles immediately. Use structured errors such as UnsupportedAdapter, RadioBusy, PermissionDenied, PeerExpired, Rejected, DiskFull and NetworkRestoreFailed. Add capability negotiation so newer UI features degrade safely with older backends.

Model radio, visibility and transfers separately:

- Radio: idle -> acquiring -> active -> releasing -> idle/error.
- Visibility: hidden -> receiving -> expired/hidden.
- Transfer: queued -> awaiting approval -> transferring -> finalizing -> completed/failed/cancelled.

Use a single owner/broker to arbitrate radio resources, bounded queues and idempotent stop/cancel behavior. Never fight another Owly client. Bind IPv6 and discovery traffic to the correct interface; do not infer reachability from names alone.

On lock, hide sensitive UI and stop new incoming offers. Already approved file transfers may finish under a documented policy. Honor always-on intent after unlock only through a fresh state check. Do not access or share clipboard data while locked.

Handle suspend/resume, rfkill, adapter removal, service disappearance, network changes and logout. Do not claim resumable transfers unless implemented. On restart, reconcile interrupted work, remove abandoned staging files, and report failures. Avoid infinite restart loops that repeatedly seize Wi-Fi.

On extension disable, remove all actors, signals, timers, shortcuts, D-Bus subscriptions and pending callbacks. Release extension-owned discovery/visibility leases. Independently owned transfers may continue only if their lifecycle is explicitly designed and still visible/manageable through another client. Make repeated enable/disable leak-free.

## Security, permissions and performance

- Retain explicit incoming approval; no trust based only on device names or recent history.
- Verify upstream archive defenses. Reject path traversal, unsafe symlinks/hardlinks and writes outside staging; bound decompression, file counts, total size and queue depth.
- Handle disk-full, permissions and partial writes honestly. Finalize safely without overwriting unrelated files, including across filesystems.
- Escape untrusted names in UI markup; sanitize filenames without losing collision safety.
- Keep received data and clipboard processing unprivileged. Validate D-Bus callers/arguments and existing service policy; a user session bus is not a security boundary against all same-user processes.
- Do not disable SELinux, AppArmor, firewall protections or regulatory checks to make tests pass.
- Restrict firewall changes to verified required interfaces, protocols and ports. Upstream documentation previously identified TCP 8771 on awdl0; verify current code and discovery requirements before implementing rules.
- Default diagnostics to redacted metadata. Never include clipboard content, payload bytes or unnecessary personal paths.
- No synchronous network/filesystem work on the Shell main thread. Throttle progress signals initially to about four per second, bound peer lists/previews and avoid tight polling.
- No continuous radio discovery while Off. Measure service memory, idle CPU and Shell responsiveness; publish measured results instead of unsupported claims.

## Packaging and compatibility

Deliver an Owly backend plus extension installation experience. Reuse upstream packages where compatible. If a bridge is needed, bundle it in a clear native backend package rather than making users assemble undocumented components.

Suggested package split: backend/bridge, GNOME extension, optional Nautilus integration, and a convenience metapackage. Keep auxiliary dialogs with the smallest appropriate package.

- Fedora: build RPMs; validate NetworkManager, firewalld and SELinux enforcing.
- Ubuntu: build DEBs; validate supported dependencies, NetworkManager, UFW where enabled, and applicable AppArmor policy.
- Test clean install, upgrade, API mismatch, downgrade where supported, and uninstall. Preserve user data and unrelated firewall rules.
- A GNOME Extensions installation alone cannot be assumed to install a system backend. Show a clear Owly backend required state and appropriate setup instructions.
- The extensions.gnome.org archive must not bundle native executables/libraries. Ship reviewable JavaScript, schemas and assets; verify current review requirements before submission.
- Native packages are the initial route. Flatpak-only radio integration, Atomic desktop workflows and extra distributions are follow-up work.

Initial proposed targets: Fedora 44/GNOME 50 and Ubuntu 26.04/GNOME 50; add GNOME 51 compatibility on relevant Fedora/Ubuntu releases. Verify actual release combinations and dependency versions before claiming support. Ubuntu 24.04/GNOME 46 is a later compatibility target. Do not declare every Shell version compatible without testing. Older Ubuntu backend compatibility does not automatically imply extension compatibility.

Use separate compatibility modules only where APIs actually differ. Avoid a legacy Shell implementation unless specifically requested.

## Milestones and acceptance gates

### M0 — Audit and real-device feasibility

Pin upstream, record the usable API, identify missing hooks, and prove explicit approval, cancellation, discovery and radio recovery. Test the user's exclusive Intel configuration when hardware access is available. Prepare a short architecture decision record explaining reuse, bridge or fork choice.

Exit: repeated bidirectional real-device transfers and network restoration demonstrated, or precise hardware blockers recorded while software-only work proceeds. Never present the latter as hardware acceptance.

### M1 — Backend contract

Implement the API adapter, capability/version checks, state machines, bounded queue, structured errors and mock backend. Add meaningful lifecycle and payload tests.

Exit: send, receive, accept/reject, cancel, service failure and radio release can be exercised through the contract without terminal-output scraping.

### M2 — Native extension alpha

Implement the QS tile, nearby devices, all three receiving modes, file picker, exclusive-mode consent, incoming approval, progress and notifications. Missing backend and unsupported hardware must have useful UI states.

Exit: a user can send and receive files without a terminal after installation, with no Shell blocking or misleading receiving state.

### M3 — Clipboard and desktop integration

Add text/link sharing, image/copied-file handling, optional Nautilus sharing and basic Copyous history launching. Pursue a direct Copyous action only through a supported interface.

Exit: core clipboard sharing works with Copyous absent, present and disabled; only explicitly chosen data is sent.

### M4 — Fedora and Ubuntu beta

Build packages, service setup and narrowly scoped policy integration. Exercise target Shell versions, permissions and clean install/upgrade/uninstall paths.

Exit: reproducible package installs on the primary target distributions with no disabled security controls or undocumented privileged steps.

### M5 — Release hardening

Complete recovery, accessibility, scaling and compatibility tests; add translations infrastructure, setup/troubleshooting docs and an accurate hardware support table. Prepare release artifacts and an extension submission package; publication requires separate authorization.

Exit: release gates below pass, limitations are documented, and no mocked result is described as a hardware test.

Indicative effort for one experienced developer with hardware access is 9–14 weeks across these milestones, not a deadline or a reason to defer useful implementation. Prioritize a working vertical slice before broad integration.

## Test matrix and definition of done

Use unit/contract tests and mocked D-Bus services for software behavior, actual GNOME sessions for UI lifecycle, clean distro environments for packaging, and physical adapters/Apple devices for radio interoperability. Containers cannot establish Wi-Fi compatibility. Use actual runtime requirements rather than assumptions about nested Shell support.

Required coverage:

- Shared, exclusive, dedicated second adapter, unsupported and untested hardware paths.
- Tested Apple device/OS versions explicitly recorded; macOS/iPad support advertised only after testing.
- Empty files, Unicode/duplicate names, many files, large transfers, rejection, cancellation and peers disappearing.
- Text, multiline code, URL, image, copied-file lists, empty clipboard and unsupported formats.
- Lock/unlock, suspend/resume, logout, backend crash, extension disable/re-enable and concurrent client ownership.
- AP changes, rfkill, adapter removal, firewall enabled and networking restoration failure.
- Disk full, read-only destinations, unsafe archive contents, interrupted finalization and staging cleanup.
- Keyboard navigation, screen reader labels, scaling, light/dark/high-contrast themes.

Release gates:

1. At least 20 consecutive bidirectional transfer cycles on each primary tested hardware configuration without leaked radio ownership.
2. Tested stop/cancel/crash paths restore networking or visibly report a specific recovery failure.
3. No incoming payload accepted without the required approval, and no clipboard payload transmitted without explicit interaction.
4. Repeated extension enable/disable leaves no accumulating actors, timers, subscriptions or errors.
5. Always-on receiving, expiry and Off behave correctly while transfers are active.
6. Supported packages install and upgrade cleanly; backend mismatch fails gracefully.
7. Discovery latency and throughput are reported as measurements per hardware/OS combination, not universal promises.

Do not add superficial tests that merely restate implementation. Prioritize state transitions, authority boundaries, cancellation races, malformed payloads, file safety and recovery.

## Required deliverables and handoff

- Working extension and backend integration, with only necessary upstream patches/fork changes.
- Reproducible build commands and pinned dependencies.
- RPM/DEB artifacts or reproducible packaging definitions, plus extension ZIP.
- Optional integrations that do not become mandatory core dependencies.
- README covering installation, usage, exclusive-mode implications and receiving modes.
- Architecture/API documentation, upstream divergence notes, compatibility matrix and honest test results.
- Diagnostics/troubleshooting for backend missing, radio busy, unsupported card, slow channel-6 operation, firewall failures and network restoration failures.
- A final report listing implemented features, checks performed, unverified hardware behavior and remaining work.

The first meaningful demonstration is: open the GNOME AirDrop menu, select a discoverable device, send a file, accept a file back, and return the adapter to its previous networking state. The finished product adds receiving modes, clipboard sharing and desktop integrations without compromising that baseline.

## Primary references to revalidate

- Owly source and installation: https://github.com/t4t5/owly
- Owly protocol specification: https://github.com/t4t5/owly/blob/main/docs/SPEC.md
- Copyous source and documented integration: https://github.com/boerdereinar/copyous
- GNOME Quick Settings: https://gjs.guide/extensions/topics/quick-settings.html
- GNOME extension review requirements: https://gjs.guide/extensions/review-guidelines/review-guidelines.html

This file is the product and implementation brief. URLs and upstream code are evidence to inspect, not permission to replace the requested scope or silently trust unverified capabilities.
