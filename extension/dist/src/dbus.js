import Gio from 'gi://Gio';
export const AIRDROP_BUS_NAME = 'io.github.t4t5.AirDropGnome';
export const AIRDROP_OBJECT_PATH = '/io/github/t4t5/AirDropGnome';
export const AIRDROP_INTERFACE = 'io.github.t4t5.AirDropGnome1';
export const AirDropIfaceXml = `
<node>
  <interface name="io.github.t4t5.AirDropGnome1">
    <method name="GetVersion">
      <arg type="u" direction="out" name="api_version"/>
      <arg type="s" direction="out" name="bridge_version"/>
      <arg type="a{sv}" direction="out" name="capabilities"/>
    </method>
    <method name="GetRadioState">
      <arg type="a{sv}" direction="out" name="state"/>
    </method>
    <method name="SetReceiving">
      <arg type="s" direction="in" name="mode"/>
      <arg type="u" direction="in" name="duration_secs"/>
    </method>
    <method name="Browse"/>
    <method name="SendFiles">
      <arg type="as" direction="in" name="files"/>
      <arg type="s" direction="in" name="peer_id"/>
      <arg type="u" direction="out" name="transfer_id"/>
    </method>
    <method name="CancelTransfer">
      <arg type="u" direction="in" name="transfer_id"/>
    </method>
    <method name="AcceptIncoming">
      <arg type="u" direction="in" name="offer_id"/>
    </method>
    <method name="RejectIncoming">
      <arg type="u" direction="in" name="offer_id"/>
    </method>
    <method name="GetTransfers">
      <arg type="aa{sv}" direction="out" name="transfers"/>
    </method>
    <method name="GetPeers">
      <arg type="aa{sv}" direction="out" name="peers"/>
    </method>
    <method name="GetDiagnostics">
      <arg type="s" direction="out" name="diagnostics_json"/>
    </method>

    <signal name="NotifyRadioStateChanged">
      <arg type="a{sv}" name="state"/>
    </signal>
    <signal name="NotifyPeerAdded">
      <arg type="a{sv}" name="peer"/>
    </signal>
    <signal name="NotifyPeerRemoved">
      <arg type="s" name="peer_id"/>
    </signal>
    <signal name="NotifyPeerChanged">
      <arg type="a{sv}" name="peer"/>
    </signal>
    <signal name="NotifyIncomingOffer">
      <arg type="a{sv}" name="offer"/>
    </signal>
    <signal name="NotifyTransferProgress">
      <arg type="a{sv}" name="transfer"/>
    </signal>
    <signal name="NotifyTransferCompleted">
      <arg type="a{sv}" name="transfer"/>
    </signal>
    <signal name="NotifyTransferFailed">
      <arg type="a{sv}" name="transfer"/>
    </signal>
    <signal name="NotifyTransferCancelled">
      <arg type="u" name="transfer_id"/>
    </signal>

    <property name="ApiVersion" type="u" access="read"/>
    <property name="CurrentRadioState" type="s" access="read"/>
    <property name="ReceivingMode" type="s" access="read"/>
    <property name="ReceivingExpiry" type="t" access="read"/>
    <property name="BackendAvailable" type="b" access="read"/>
    <property name="AdapterReady" type="b" access="read"/>
  </interface>
</node>`;
export const AirDropProxy = Gio.DBusProxy.makeProxyWrapper(AirDropIfaceXml);
