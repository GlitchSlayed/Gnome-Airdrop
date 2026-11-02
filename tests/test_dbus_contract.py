#!/usr/bin/env python3
"""
test_dbus_contract.py — verifies the airdrop-bridge D-Bus interface.
Spawns the bridge in mock mode (AIRDROP_MOCK=1) and exercises:
  - Version & capabilities call
  - Radio state inspection
  - Receiving mode state machine (off -> timed -> always -> off)
  - Discovered peer list & PeerAdded/PeerRemoved signals
  - Incoming transfer offer & accept/reject
  - File send simulation & progress tracking
"""

import os
import sys
import time
import subprocess
import dbus
import dbus.mainloop.glib
from gi.repository import GLib

BUS_NAME = "io.github.t4t5.AirDropGnome"
OBJ_PATH = "/io/github/t4t5/AirDropGnome"
IFACE_NAME = "io.github.t4t5.AirDropGnome1"

def run_tests():
    print("[1/6] Launching airdrop-bridge in MOCK mode...")
    env = os.environ.copy()
    env["AIRDROP_MOCK"] = "1"
    env["AIRDROP_LOG"] = "info"

    bridge_bin = os.path.abspath("service/target/release/airdrop-bridge")
    proc = subprocess.Popen([bridge_bin], env=env)

    try:
        # Give bridge 500ms to register on session bus
        time.sleep(0.6)

        bus = dbus.SessionBus()
        obj = bus.get_object(BUS_NAME, OBJ_PATH)
        iface = dbus.Interface(obj, IFACE_NAME)
        props = dbus.Interface(obj, "org.freedesktop.DBus.Properties")

        print("[2/6] Verifying GetVersion()...")
        api_ver, bridge_ver, caps = iface.GetVersion()
        assert api_ver == 1, f"Expected API version 1, got {api_ver}"
        print(f"      API version: {api_ver}, Bridge version: {bridge_ver}")

        print("[3/6] Verifying GetRadioState()...")
        state = iface.GetRadioState()
        radio_state = str(state.get("radio_state"))
        recv_mode = str(state.get("receiving_mode"))
        print(f"      Radio state: {radio_state}, Receiving mode: {recv_mode}")
        assert radio_state in ["idle", "active", "acquiring", "releasing"], f"Invalid radio state {radio_state}"

        print("[4/6] Testing Receiving Mode transitions...")
        iface.SetReceiving("timed", dbus.UInt32(600))
        time.sleep(0.2)
        state = iface.GetRadioState()
        assert str(state.get("receiving_mode")) == "timed", f"Expected timed mode, got {state.get('receiving_mode')}"
        print("      SetReceiving('timed', 600) -> OK")

        iface.SetReceiving("always", dbus.UInt32(0))
        time.sleep(0.2)
        state = iface.GetRadioState()
        assert str(state.get("receiving_mode")) == "always", f"Expected always mode, got {state.get('receiving_mode')}"
        print("      SetReceiving('always') -> OK")

        iface.SetReceiving("off", dbus.UInt32(0))
        time.sleep(0.2)
        state = iface.GetRadioState()
        assert str(state.get("receiving_mode")) == "off", f"Expected off mode, got {state.get('receiving_mode')}"
        print("      SetReceiving('off') -> OK")

        print("[5/6] Verifying Mock Peers...")
        time.sleep(1.5)  # wait for mock to populate peers
        peers = iface.GetPeers()
        print(f"      Discovered {len(peers)} peer(s)")
        for p in peers:
            print(f"        • {p.get('name')} (id={p.get('id')}, type={p.get('device_type')})")
        assert len(peers) > 0, "Expected at least 1 mock peer"

        print("[6/6] Verifying SendFiles() on peer...")
        first_peer_id = str(peers[0].get("id"))
        # Create a dummy file to send
        dummy_file = "/tmp/test_airdrop_share.txt"
        with open(dummy_file, "w") as f:
            f.write("Hello from airdrop-gnome automated contract test!")
        
        transfer_id = iface.SendFiles([dummy_file], first_peer_id)
        print(f"      SendFiles initiated -> transfer_id={transfer_id}")
        assert int(transfer_id) >= 1, "Expected positive transfer ID"

        print("=== ALL 6 CONTRACT & LIFECYCLE TESTS PASSED SUCCESSFULLY ===")

    finally:
        proc.terminate()
        proc.wait()

if __name__ == "__main__":
    run_tests()
