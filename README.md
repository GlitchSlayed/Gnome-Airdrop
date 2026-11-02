# AirDrop for GNOME (`airdrop-gnome`)

A polished GNOME Shell extension that brings real AirDrop sharing into the Quick Settings menu on Fedora and Ubuntu, powered by [Owly](https://github.com/t4t5/owly).

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![GNOME Shell](https://img.shields.io/badge/GNOME%20Shell-50%20%7C%2051-green.svg)

---

## Features

- **Native Quick Settings Integration**: Seamless toggle and popup menu matching GNOME HIG.
- **Discover Nearby AirDrop Devices**: Scans and displays Apple devices (iPhones, Macs, iPads) with device-type symbolic icons.
- **File Sharing**: Native file picker integration to select and send files directly to a discovered device.
- **Receiving Visibility Modes**:
  - **Off**: Radio stays idle, Wi-Fi operates normally.
  - **Everyone for 10 Minutes**: Timed discoverability window.
  - **Always On**: Continuously discoverable (each incoming transfer still requires explicit user approval).
- **Explicit Incoming Approval**: Notifications and interactive Accept/Reject actions for incoming file transfers.
- **Hardware-Aware & Safe**: Dynamically detects your Wi-Fi card and operating mode. On cards requiring exclusive mode (such as Intel cards), prompts for explicit user consent before taking the adapter away from standard Wi-Fi, and automatically restores normal Wi-Fi when finished.
- **Mock Mode**: Built-in mock mode (`AIRDROP_MOCK=1`) for testing and UI development without disconnecting Wi-Fi.

---

## Architecture

```
┌────────────────────────────────────────┐
│         GNOME Shell Extension          │
│   (Quick Settings Tile & Native Menu)  │
└───────────────────▲────────────────────┘
                    │ D-Bus (Session Bus: io.github.t4t5.AirDropGnome)
┌───────────────────▼────────────────────┐
│      airdrop-bridge (Rust Daemon)      │
│  State machines, mDNS browse & wrap    │
└───────────▲──────────────────▲─────────┘
            │                  │
    Subprocess spawn    System D-Bus (io.github.t4t5.Owly)
            │                  │
   ┌────────▼───────┐   ┌──────▼───────┐
   │ owly CLI       │   │ owlyd        │
   │ AirDrop HTTP   │   │ Radio / AWDL │
   └────────────────┘   └──────────────┘
```

1. **`owlyd` (System Service)**: Upstream privileged service managing AWDL radio acquisition.
2. **`airdrop-bridge` (User Session Service)**: Unprivileged Rust daemon holding the radio lease and orchestrating mDNS discovery and AirDrop HTTP transfers.
3. **GNOME Shell Extension (TypeScript / GJS)**: UI frontend adhering strictly to GNOME review guidelines (asynchronous D-Bus communication, no long-running or blocking main thread execution).

---

## Getting Started

### 1. Requirements

- Fedora 44+ / Ubuntu 26.04+ (GNOME Shell 50 or 51)
- [Owly](https://github.com/t4t5/owly) (version 0.4.2+) installed on the system
- Rust toolchain (`cargo`, `rustc`)
- Node.js & TypeScript (`tsc`)

### 2. Build

```bash
# 1. Build the Rust session bridge
cd service
cargo build --release
cd ..

# 2. Compile the GNOME Shell extension
cd extension
npm install
npx tsc
glib-compile-schemas schemas/
cd ..

# 3. Package extension ZIP
mkdir -p build/extension-pkg
cp -r extension/dist/* build/extension-pkg/
cp extension/metadata.json build/extension-pkg/
mkdir -p build/extension-pkg/schemas build/extension-pkg/icons
cp extension/schemas/* build/extension-pkg/schemas/
cp extension/icons/* build/extension-pkg/icons/
cd build/extension-pkg && zip -r ../airdrop-gnome@vansh.zip * && cd ../..
```

### 3. Run and Install

#### Install Extension
```bash
gnome-extensions install build/airdrop-gnome@vansh.zip --force
gnome-extensions enable airdrop-gnome@vansh
```

#### Run the Bridge
```bash
# Production mode (connects to real owlyd and Wi-Fi adapter):
./service/target/release/airdrop-bridge

# Or mock mode (safe testing without hardware/Wi-Fi impact):
AIRDROP_MOCK=1 ./service/target/release/airdrop-bridge
```

---

## Testing

Run the automated D-Bus contract verification suite:

```bash
python3 tests/test_dbus_contract.py
```

---

## Hardware Support Notes

- **Intel Wi-Fi (iwlwifi)**: Tested on Intel AX200 (`8086:2723`). Operates in exclusive mode on channel 6. Normal Wi-Fi will disconnect while an active AirDrop operation or visibility window is open, and restores upon completion.
- **MediaTek / Realtek**: Supported in shared mode or exclusive mode depending on the specific chipset. Run `owly doctor` to inspect your card's capabilities.

---

## License

This project is licensed under the [MIT License](LICENSE).
Owly is copyright (c) t4t5 and licensed under the MIT License.
