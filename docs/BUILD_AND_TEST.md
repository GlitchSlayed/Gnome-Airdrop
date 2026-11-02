# Build and Run Guide — airdrop-gnome

## Overview

`airdrop-gnome` integrates AirDrop into the GNOME Shell Quick Settings menu using:
- **`owlyd`**: Upstream system daemon handling privileged AWDL radio acquisition.
- **`airdrop-bridge`**: An unprivileged session D-Bus bridge that communicates with `owlyd`, drives mDNS discovery, and wraps AirDrop transfers.
- **GNOME Shell Extension**: Native Quick Settings tile and menu written in TypeScript/modern GJS.

---

## 1. Prerequisites

- Fedora 44+ / Ubuntu 26.04+ (or compatible GNOME Shell 50/51)
- Rust 1.88+ (`cargo`, `rustc`)
- Node.js & TypeScript (`npx tsc`)
- Upstream `owly` (0.4.2+) installed:
  ```bash
  # Check owlyd system service status
  systemctl status owlyd
  ```

---

## 2. Building the Components

### Build the Session Bridge (`airdrop-bridge`)
```bash
cd service
cargo build --release
# Binary produced at: service/target/release/airdrop-bridge
```

### Compile the GNOME Shell Extension
```bash
cd extension
npm install
npx tsc
glib-compile-schemas schemas/
```

### Package Extension ZIP
```bash
mkdir -p build/extension-pkg
cd build/extension-pkg
cp -r ../../extension/dist/* .
cp ../../extension/metadata.json .
mkdir -p schemas icons
cp ../../extension/schemas/* schemas/
cp ../../extension/icons/* icons/
zip -r ../airdrop-gnome@vansh.zip *
```

---

## 3. Running & Testing

### Option A: Mock Mode (No Hardware / Radio Disconnect Required)
Test the entire UI and D-Bus lifecycle without disconnecting your Wi-Fi:

1. In a terminal, run the bridge with `AIRDROP_MOCK=1`:
   ```bash
   AIRDROP_MOCK=1 ./service/target/release/airdrop-bridge
   ```
2. Run the automated contract verification suite:
   ```bash
   python3 tests/test_dbus_contract.py
   ```

### Option B: Real Hardware Mode (Owly + Intel AX200 Exclusive Mode)
1. Ensure `owlyd` is running:
   ```bash
   systemctl is-active owlyd
   ```
2. Start the bridge:
   ```bash
   ./service/target/release/airdrop-bridge
   ```
3. Install and enable the extension:
   ```bash
   gnome-extensions install build/airdrop-gnome@vansh.zip --force
   gnome-extensions enable airdrop-gnome@vansh
   ```
4. Click the AirDrop icon in the GNOME Quick Settings menu to discover nearby recipients, send files, or turn on receiving.
