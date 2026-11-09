import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk';
import { ExtensionPreferences } from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

import { AirDropProxy, AIRDROP_BUS_NAME, AIRDROP_OBJECT_PATH } from './dbus.js';

export default class AirDropPreferences extends ExtensionPreferences {
  [key: string]: any;

  fillPreferencesWindow(window) {
    const settings = this.getSettings();

    // ─────────────────────────────────────────────────────────────
    // Page 1: General Preferences
    // ─────────────────────────────────────────────────────────────
    const generalPage = new Adw.PreferencesPage({
      title: 'General',
      iconName: 'preferences-system-symbolic',
    });
    window.add(generalPage);

    // Group 1: Device Identity
    const identityGroup = new Adw.PreferencesGroup({
      title: 'Identity & Discoverability',
      description: 'Configure how your computer appears to other AirDrop devices.',
    });
    generalPage.add(identityGroup);

    // Device Display Name Row
    const nameRow = new Adw.EntryRow({
      title: 'Device Name',
      text: settings.get_string('display-name') || GLib.get_host_name(),
    });
    nameRow.connect('changed', (entry) => {
      settings.set_string('display-name', entry.text.trim());
    });
    identityGroup.add(nameRow);

    // Download Directory Row
    const dirRow = new Adw.ActionRow({
      title: 'Download Folder',
      subtitle: settings.get_string('receive-directory') || GLib.get_user_special_dir(GLib.UserDirectory.DIRECTORY_DOWNLOAD),
    });
    const chooseDirBtn = new Gtk.Button({
      label: 'Change…',
      valign: Gtk.Align.CENTER,
    });
    chooseDirBtn.connect('clicked', () => {
      const dialog = new Gtk.FileDialog({
        title: 'Choose AirDrop Downloads Folder',
      });
      dialog.select_folder(window, null, (_d, res) => {
        try {
          const folder = dialog.select_folder_finish(res);
          if (folder) {
            const path = folder.get_path();
            settings.set_string('receive-directory', path);
            dirRow.subtitle = path;
          }
        } catch (_) {}
      });
    });
    dirRow.add_suffix(chooseDirBtn);
    identityGroup.add(dirRow);

    // Group 2: Notifications & Session Behavior
    const behaviorGroup = new Adw.PreferencesGroup({
      title: 'Behavior & Sessions',
      description: 'Transfer alerts and persistence behavior.',
    });
    generalPage.add(behaviorGroup);

    // Notify switch
    const notifyRow = new Adw.SwitchRow({
      title: 'Show Notifications',
      subtitle: 'Display system popups when nearby devices request file transfers.',
      active: settings.get_boolean('notify-on-discover'),
    });
    notifyRow.connect('notify::active', () => {
      settings.set_boolean('notify-on-discover', notifyRow.active);
    });
    behaviorGroup.add(notifyRow);

    // Auto-restore Always-on switch
    const restoreRow = new Adw.SwitchRow({
      title: 'Restore Always-On at Login',
      subtitle: 'Keep receiving active across desktop logins. Not recommended on battery or single-adapter setups.',
      active: settings.get_boolean('auto-restore-always-on'),
    });
    restoreRow.connect('notify::active', () => {
      settings.set_boolean('auto-restore-always-on', restoreRow.active);
    });
    behaviorGroup.add(restoreRow);

    // Exclusive consent reset row
    const consentRow = new Adw.ActionRow({
      title: 'Exclusive Wi-Fi Consent',
      subtitle: settings.get_boolean('exclusive-consent-given')
        ? 'Consent granted (Wi-Fi will temporarily disconnect during AirDrop).'
        : 'Consent not yet granted.',
    });
    const resetConsentBtn = new Gtk.Button({
      label: 'Reset Consent',
      valign: Gtk.Align.CENTER,
    });
    resetConsentBtn.connect('clicked', () => {
      settings.set_boolean('exclusive-consent-given', false);
      consentRow.subtitle = 'Consent not yet granted.';
    });
    consentRow.add_suffix(resetConsentBtn);
    behaviorGroup.add(consentRow);

    // ─────────────────────────────────────────────────────────────
    // Page 2: Diagnostics & Hardware
    // ─────────────────────────────────────────────────────────────
    const diagPage = new Adw.PreferencesPage({
      title: 'Hardware & Diagnostics',
      iconName: 'network-wireless-symbolic',
    });
    window.add(diagPage);

    const diagGroup = new Adw.PreferencesGroup({
      title: 'Wi-Fi Adapter Capabilities',
      description: 'Diagnostic report retrieved directly from Owly radio service.',
    });
    diagPage.add(diagGroup);

    const stationRow = new Adw.ActionRow({ title: 'Interface', subtitle: 'Probing…' });
    const driverRow = new Adw.ActionRow({ title: 'Driver / Chipset', subtitle: 'Probing…' });
    const modeRow = new Adw.ActionRow({ title: 'Operating Mode', subtitle: 'Probing…' });
    const statusRow = new Adw.ActionRow({ title: 'Hardware Status', subtitle: 'Probing…' });

    diagGroup.add(stationRow);
    diagGroup.add(driverRow);
    diagGroup.add(modeRow);
    diagGroup.add(statusRow);

    // Fetch live hardware diagnostics over session D-Bus
    try {
      const proxy = new AirDropProxy(
        Gio.DBus.session,
        AIRDROP_BUS_NAME,
        AIRDROP_OBJECT_PATH,
        (p, err) => {
          if (!err && p) {
            p.GetDiagnosticsRemote((res, callErr) => {
              if (!callErr && res && res[0]) {
                try {
                  const data = JSON.parse(res[0]);
                  stationRow.subtitle = `${data.station || 'Unknown'} (${data.device || ''})`;
                  driverRow.subtitle = data.driver || 'Unknown';
                  modeRow.subtitle = `${data.mode || 'Unknown'} (Channel ${data.channel || '?'})`;
                  statusRow.subtitle = data.status === 'tested' ? 'Tested & Ready ✅' : (data.status || 'Unknown');
                } catch (e) {
                  statusRow.subtitle = 'Error parsing diagnostics';
                }
              }
            });
          } else {
            statusRow.subtitle = 'Service offline (airdrop-bridge not responding)';
          }
        }
      );
    } catch (e) {
      statusRow.subtitle = 'Could not contact session bus';
    }
  }
}
