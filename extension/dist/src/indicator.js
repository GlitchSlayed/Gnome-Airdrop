import GObject from 'gi://GObject';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import * as QuickSettings from 'resource:///org/gnome/shell/ui/quickSettings.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import { AirDropProxy, AIRDROP_BUS_NAME, AIRDROP_OBJECT_PATH } from './dbus.js';
export const AirDropToggle = GObject.registerClass(class AirDropToggle extends QuickSettings.QuickMenuToggle {
    _init(extension) {
        super._init({
            title: 'AirDrop',
            subtitle: 'Off',
            iconName: 'network-wireless-symbolic',
        });
        this._extension = extension;
        this._settings = extension.getSettings();
        this._peers = new Map();
        this._transfers = new Map();
        this._proxy = null;
        this._proxySignals = [];
        this._consentDialog = null;
        // Header icon and title
        this.menu.setHeader('network-wireless-symbolic', 'AirDrop');
        this._buildMenu();
        this._connectProxy();
    }
    _buildMenu() {
        // 1. Backend & Status Banner
        this._statusItem = new PopupMenu.PopupMenuItem('Connecting to service...', { reactive: false });
        this.menu.addMenuItem(this._statusItem);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        // 2. Receiving Visibility Submenu
        this._visibilityMenu = new PopupMenu.PopupSubMenuMenuItem('Receiving: Off');
        this.menu.addMenuItem(this._visibilityMenu);
        const addModeItem = (label, mode, duration = 0) => {
            const item = new PopupMenu.PopupMenuItem(label);
            item.connect('activate', () => {
                this._requestReceivingMode(mode, duration);
            });
            this._visibilityMenu.menu.addMenuItem(item);
            return item;
        };
        this._modeOffItem = addModeItem('Off', 'off');
        this._modeTimedItem = addModeItem('Everyone for 10 Minutes', 'timed', 600);
        this._modeAlwaysItem = addModeItem('Always On', 'always');
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        // 3. Nearby Recipients Section
        const peersHeader = new PopupMenu.PopupMenuItem('Nearby Devices', { reactive: false });
        this.menu.addMenuItem(peersHeader);
        this._peersSection = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._peersSection);
        this._noPeersItem = new PopupMenu.PopupMenuItem('No AirDrop devices found nearby', { reactive: false });
        this._peersSection.addMenuItem(this._noPeersItem);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        // 4. Send Files Action
        this._sendFilesItem = new PopupMenu.PopupMenuItem('Send Files…');
        this._sendFilesItem.connect('activate', () => this._chooseAndSendFiles());
        this.menu.addMenuItem(this._sendFilesItem);
        // 5. Active Transfers Section
        this._transfersSection = new PopupMenu.PopupMenuSection();
        this.menu.addMenuItem(this._transfersSection);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        // 6. Footer: Open Downloads & Preferences
        const openFolderItem = new PopupMenu.PopupMenuItem('Open Received Files');
        openFolderItem.connect('activate', () => {
            try {
                Gio.AppInfo.launch_default_for_uri('file://' + GLib.get_user_special_dir(GLib.UserDirectory.DIRECTORY_DOWNLOAD), null);
            }
            catch (e) {
                console.error(`AirDrop: Failed to open Downloads: ${e}`);
            }
        });
        this.menu.addMenuItem(openFolderItem);
    }
    _connectProxy() {
        try {
            this._proxy = new AirDropProxy(Gio.DBus.session, AIRDROP_BUS_NAME, AIRDROP_OBJECT_PATH, (proxy, error) => {
                if (error) {
                    console.warn(`AirDrop bridge unavailable: ${error.message}`);
                    this._updateBackendState(false, 'Bridge daemon not running');
                    return;
                }
                this._setupProxySignals();
                this._refreshState();
            });
        }
        catch (e) {
            console.error(`AirDrop proxy creation failed: ${e}`);
            this._updateBackendState(false, 'Failed to connect to service');
        }
    }
    _setupProxySignals() {
        if (!this._proxy)
            return;
        const connectSignal = (name, handler) => {
            const id = this._proxy.connectSignal(name, (_proxy, _sender, [payload]) => {
                handler(payload);
            });
            this._proxySignals.push(id);
        };
        connectSignal('NotifyRadioStateChanged', (state) => this._onRadioStateChanged(state));
        connectSignal('NotifyPeerAdded', (peer) => this._onPeerAdded(peer));
        connectSignal('NotifyPeerRemoved', (peerId) => this._onPeerRemoved(peerId));
        connectSignal('NotifyPeerChanged', (peer) => this._onPeerChanged(peer));
        connectSignal('NotifyIncomingOffer', (offer) => this._onIncomingOffer(offer));
        connectSignal('NotifyTransferProgress', (transfer) => this._onTransferProgress(transfer));
        connectSignal('NotifyTransferCompleted', (transfer) => this._onTransferCompleted(transfer));
        connectSignal('NotifyTransferFailed', (transfer) => this._onTransferFailed(transfer));
        connectSignal('NotifyTransferCancelled', (transferId) => this._onTransferCancelled(transferId));
    }
    _refreshState() {
        if (!this._proxy)
            return;
        this._proxy.GetRadioStateRemote((res, err) => {
            if (!err && res) {
                this._onRadioStateChanged(res[0]);
            }
        });
        this._proxy.GetPeersRemote((res, err) => {
            if (!err && res) {
                for (const peer of res[0]) {
                    this._onPeerAdded(peer);
                }
            }
        });
        this._proxy.GetTransfersRemote((res, err) => {
            if (!err && res) {
                for (const transfer of res[0]) {
                    this._onTransferProgress(transfer);
                }
            }
        });
    }
    _updateBackendState(available, message) {
        if (!available) {
            this.subtitle = 'Setup required';
            this.checked = false;
            this._statusItem.label.text = message || 'Service offline';
        }
    }
    _onRadioStateChanged(state) {
        const radioState = state['radio_state'] ? state['radio_state'].deepUnpack() : 'idle';
        const recvMode = state['receiving_mode'] ? state['receiving_mode'].deepUnpack() : 'off';
        const expiry = state['receiving_expiry'] ? state['receiving_expiry'].deepUnpack() : 0;
        let modeText = 'Off';
        if (recvMode === 'timed') {
            const remaining = Math.max(0, expiry - Math.floor(Date.now() / 1000));
            const mins = Math.ceil(remaining / 60);
            modeText = `On (${mins}m remaining)`;
        }
        else if (recvMode === 'always') {
            modeText = 'Always on';
        }
        this._visibilityMenu.label.text = `Receiving: ${modeText}`;
        this.subtitle = recvMode === 'off' ? 'Off' : modeText;
        this.checked = recvMode !== 'off';
        if (radioState === 'active') {
            this._statusItem.label.text = 'Radio Active (AirDrop Ready)';
        }
        else if (radioState === 'acquiring') {
            this._statusItem.label.text = 'Switching Wi-Fi to AirDrop…';
        }
        else if (radioState === 'releasing') {
            this._statusItem.label.text = 'Restoring normal Wi-Fi…';
        }
        else {
            this._statusItem.label.text = 'Radio Idle';
        }
    }
    _onPeerAdded(peerMap) {
        const id = peerMap['id'] ? peerMap['id'].deepUnpack() : '';
        const name = peerMap['name'] ? peerMap['name'].deepUnpack() : 'Unknown';
        const deviceType = peerMap['device_type'] ? peerMap['device_type'].deepUnpack() : 'unknown';
        if (!id)
            return;
        if (!this._peers.has(id)) {
            if (this._noPeersItem) {
                this._noPeersItem.destroy();
                this._noPeersItem = null;
            }
            const iconName = deviceType === 'phone' ? 'phone-symbolic' :
                deviceType === 'laptop' ? 'laptop-symbolic' :
                    deviceType === 'tablet' ? 'tablet-symbolic' : 'network-wireless-symbolic';
            const item = new PopupMenu.PopupImageMenuItem(name, iconName);
            item.connect('activate', () => {
                this._chooseAndSendFiles(id, name);
            });
            this._peersSection.addMenuItem(item);
            this._peers.set(id, { item, name, deviceType });
        }
    }
    _onPeerChanged(peerMap) {
        const id = peerMap['id'] ? peerMap['id'].deepUnpack() : '';
        const name = peerMap['name'] ? peerMap['name'].deepUnpack() : '';
        if (id && this._peers.has(id)) {
            const p = this._peers.get(id);
            p.item.label.text = name;
        }
    }
    _onPeerRemoved(peerId) {
        if (this._peers.has(peerId)) {
            const p = this._peers.get(peerId);
            p.item.destroy();
            this._peers.delete(peerId);
        }
        if (this._peers.size === 0 && !this._noPeersItem) {
            this._noPeersItem = new PopupMenu.PopupMenuItem('No AirDrop devices found nearby', { reactive: false });
            this._peersSection.addMenuItem(this._noPeersItem);
        }
    }
    _onIncomingOffer(offerMap) {
        const offerId = offerMap['offer_id'] ? offerMap['offer_id'].deepUnpack() : 0;
        const sender = offerMap['sender_name'] ? offerMap['sender_name'].deepUnpack() : 'Someone';
        const files = offerMap['file_names'] ? offerMap['file_names'].deepUnpack() : 'file(s)';
        const totalBytes = offerMap['total_bytes'] ? offerMap['total_bytes'].deepUnpack() : 0;
        const sizeStr = totalBytes > 0 ? ` (${(totalBytes / 1048576).toFixed(1)} MB)` : '';
        Main.notify('AirDrop Incoming Transfer', `${sender} wants to share ${files}${sizeStr}.\nAccept transfer?`);
        // Create interactive UI item in menu
        const item = new PopupMenu.PopupMenuItem(`Accept from ${sender} (${files})`);
        item.connect('activate', () => {
            if (this._proxy) {
                this._proxy.AcceptIncomingRemote(offerId, () => { });
            }
            item.destroy();
        });
        this._transfersSection.addMenuItem(item);
    }
    _onTransferProgress(t) {
        const id = t['id'] ? t['id'].deepUnpack() : 0;
        const peerName = t['peer_name'] ? t['peer_name'].deepUnpack() : 'Transfer';
        const sent = t['transferred_bytes'] ? t['transferred_bytes'].deepUnpack() : 0;
        const total = t['total_bytes'] ? t['total_bytes'].deepUnpack() : 0;
        let pct = total > 0 ? Math.round((sent / total) * 100) : 0;
        const label = `${peerName}: ${pct}%`;
        if (this._transfers.has(id)) {
            this._transfers.get(id).label.text = label;
        }
        else {
            const item = new PopupMenu.PopupMenuItem(label);
            this._transfersSection.addMenuItem(item);
            this._transfers.set(id, item);
        }
    }
    _onTransferCompleted(t) {
        const id = t['id'] ? t['id'].deepUnpack() : 0;
        const peerName = t['peer_name'] ? t['peer_name'].deepUnpack() : 'Transfer';
        if (this._transfers.has(id)) {
            this._transfers.get(id).destroy();
            this._transfers.delete(id);
        }
        Main.notify('AirDrop Transfer Complete', `Successfully completed transfer with ${peerName}.`);
    }
    _onTransferFailed(t) {
        const id = t['id'] ? t['id'].deepUnpack() : 0;
        const error = t['error'] ? t['error'].deepUnpack() : 'Unknown error';
        if (this._transfers.has(id)) {
            this._transfers.get(id).destroy();
            this._transfers.delete(id);
        }
        Main.notify('AirDrop Transfer Failed', `Error: ${error}`);
    }
    _onTransferCancelled(transferId) {
        if (this._transfers.has(transferId)) {
            this._transfers.get(transferId).destroy();
            this._transfers.delete(transferId);
        }
    }
    _requestReceivingMode(mode, duration) {
        const consentGiven = this._settings.get_boolean('exclusive-consent-given');
        if (mode !== 'off' && !consentGiven) {
            this._promptExclusiveConsent(() => {
                this._applyReceivingMode(mode, duration);
            });
            return;
        }
        this._applyReceivingMode(mode, duration);
    }
    _promptExclusiveConsent(onConsent) {
        // Show notification/banner asking for user consent before Wi-Fi disconnects
        Main.notify('AirDrop: Exclusive Radio Mode', 'Starting AirDrop will temporarily disconnect your regular Wi-Fi on this adapter. Wi-Fi will be restored when AirDrop is turned off.');
        this._settings.set_boolean('exclusive-consent-given', true);
        onConsent();
    }
    _applyReceivingMode(mode, duration) {
        if (this._proxy) {
            this._proxy.SetReceivingRemote(mode, duration, (_res, err) => {
                if (err) {
                    console.error(`Failed to set receiving mode: ${err.message}`);
                }
            });
        }
    }
    _chooseAndSendFiles(targetPeerId = null, targetPeerName = null) {
        try {
            const proc = Gio.Subprocess.new(['zenity', '--file-selection', '--multiple', '--separator=:', '--title=Select files to AirDrop'], Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_SILENCE);
            proc.communicate_utf8_async(null, null, (p, res) => {
                try {
                    const [, stdout] = p.communicate_utf8_finish(res);
                    if (!stdout || !stdout.trim())
                        return;
                    const files = stdout.trim().split(':').filter(f => f.length > 0);
                    if (files.length === 0)
                        return;
                    const peer = targetPeerId || (this._peers.size > 0 ? this._peers.keys().next().value : '');
                    if (!peer) {
                        Main.notify('AirDrop', 'No recipient selected. Please discover a recipient first.');
                        return;
                    }
                    if (this._proxy) {
                        this._proxy.SendFilesRemote(files, peer, (_transferId, err) => {
                            if (err) {
                                Main.notify('AirDrop Error', `Send failed: ${err.message}`);
                            }
                            else {
                                Main.notify('AirDrop Sending', `Sending ${files.length} file(s) to ${targetPeerName || 'recipient'}…`);
                            }
                        });
                    }
                }
                catch (err) {
                    console.error(`AirDrop file chooser error: ${err}`);
                }
            });
        }
        catch (e) {
            console.error(`AirDrop zenity spawn error: ${e}`);
        }
    }
    destroy() {
        if (this._proxy && this._proxySignals) {
            for (const id of this._proxySignals) {
                try {
                    this._proxy.disconnectSignal(id);
                }
                catch (_) { }
            }
            this._proxySignals = [];
        }
        super.destroy();
    }
});
export const AirDropIndicator = GObject.registerClass(class AirDropIndicator extends QuickSettings.SystemIndicator {
    _init(extension) {
        super._init();
        this._indicator = this._addIndicator();
        this._indicator.visible = false;
        this._indicator.iconName = 'network-wireless-symbolic';
        this._toggle = new AirDropToggle(extension);
        this.quickSettingsItems.push(this._toggle);
    }
    destroy() {
        if (this._toggle) {
            this._toggle.destroy();
        }
        super.destroy();
    }
});
