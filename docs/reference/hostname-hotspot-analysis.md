# Hostname & WiFi Hotspot Analysis

## 1. Hostname Configuration

### Implementation Approach

RePlayOS uses Debian Trixie with NetworkManager. Changing the hostname requires three steps:

1. **`hostnamectl set-hostname <name>`** -- updates `/etc/hostname` and the kernel hostname in one call. No reboot needed for the system hostname itself.
2. **Update `/etc/hosts`** -- replace the old hostname entry (e.g., `127.0.1.1 replay`) with the new one. Without this, `sudo` and some services log warnings.
3. **Restart Avahi** -- `systemctl restart avahi-daemon` so mDNS broadcasts the new `<name>.local` address. Existing clients may cache the old name for a few minutes.

All three can happen live. Replay Control already calls `systemctl restart replay` via a server function, so the pattern is established.

### Validation Rules

- Lowercase letters, digits, hyphens only. No leading/trailing hyphens. Max 63 chars (RFC 1123).
- Avahi publishes `<hostname>.local`, so the name must also be a valid mDNS label.

### Config Storage

The hostname is a system-level setting, not a RePlayOS game/emulation setting. Two options:

- **Option A: System-only** -- just call `hostnamectl`, don't store in `replay.cfg`. The hostname persists across reboots via `/etc/hostname`. Read current hostname with `hostname` or `hostnamectl`.
- **Option B: `replay.cfg`** -- add a `system_hostname` key. On boot (or on save), Replay Control applies it via `hostnamectl`. This keeps the setting visible/portable with the SD card (`replay.cfg` always lives at `/media/sd/config/replay.cfg`), matching how `wifi_name` already lives in `replay.cfg`.

Recommendation: **Option A** (system-only). The hostname belongs to the OS, not to the ROM library. If RePlayOS ever adds its own hostname support to `replay.cfg`, we can adapt.

### UX

- New section on the `/more` (Settings) page, or a field on a future "System" settings page.
- Text input with live validation (RFC 1123 rules), current hostname shown as placeholder.
- After saving, display the new `.local` address so the user knows how to reconnect.
- Warn that bookmarks/browser address bar will need updating.

### Complexity: Low

One server function (`set_hostname`), one small UI form, no new dependencies.

---

## 2. WiFi Hotspot Mode

### WiPi Netbooter Reference

The WiPi Netbooter (for Sega Naomi arcade boards) demonstrates this concept well. From the manual:

- On first boot, the Pi broadcasts a WiFi network called **"WiPi-Netbooter"** (password: "segarocks").
- Users join from a phone/tablet and open `http://netbooter.local` to access the web UI.
- The Pi uses a **fixed IP (10.0.0.1)** on its wired interface to talk to the arcade hardware.
- Four network modes are supported: **Hotspot Direct** (default), **Home WiFi Direct**, **Hotspot Router**, and **Home WiFi Router** -- covering combinations of hotspot vs. home WiFi and direct-wired vs. router-connected.
- The hostname is configurable, updating the `.local` address accordingly.

Key takeaway: the hotspot is the **zero-config default**. Users who want home network access can switch to WiFi client mode later. This is the right model for ReplayOS too.

### Implementation Approach

RePlayOS uses **NetworkManager** (not raw `hostapd`/`dnsmasq`). NetworkManager has built-in AP (access point) mode, which is the simplest path:

```
nmcli device wifi hotspot ifname wlan0 ssid "ReplayOS" password "replayos123"
```

This single command:
- Creates a WiFi access point on `wlan0`
- Starts a DHCP server for clients (NetworkManager uses its internal DHCP or dnsmasq)
- Assigns the Pi a gateway IP (typically `10.42.0.1`)

No need to install or configure `hostapd` or `dnsmasq` separately.

### Required Components

- **NetworkManager** (already present on RePlayOS) -- handles AP mode, DHCP, and DNS
- **No additional packages** -- `nmcli device wifi hotspot` works out of the box on Raspberry Pi OS / Debian with NetworkManager

### Configuration Parameters

| Parameter | Key in `replay.cfg` | Default | Notes |
|-----------|---------------------|---------|-------|
| Mode | `wifi_hotspot` | `"false"` | `"true"` = hotspot, `"false"` = client |
| SSID | `wifi_hotspot_ssid` | `"ReplayOS"` | Broadcast name |
| Password | `wifi_hotspot_pwd` | `"replayos123"` | WPA2 password (min 8 chars) |
| Band | `wifi_hotspot_band` | `"bg"` | `"bg"` (2.4GHz) or `"a"` (5GHz) |

The existing `wifi_name`/`wifi_pwd` keys remain for client-mode WiFi. Hotspot is a separate mode toggle.

### Switching Between Modes

- **Hotspot -> Client**: `nmcli connection down Hotspot && nmcli device wifi connect "<ssid>" password "<pwd>"`
- **Client -> Hotspot**: `nmcli device wifi hotspot ...`
- The switch is immediate but **disconnects the current session**. The UI should warn: "You will lose connection. Reconnect to the ReplayOS WiFi network."

**Single-radio constraint**: The Raspberry Pi has one WiFi radio. It cannot be a hotspot and a WiFi client simultaneously. This is the same limitation WiPi has -- it offers either/or modes. Wired ethernet remains available in both modes if connected.

### IP Addressing and DNS

- In hotspot mode, NetworkManager assigns the Pi `10.42.0.1` (default) and hands out `10.42.0.x` addresses to clients via DHCP.
- Avahi/mDNS works over the hotspot interface, so `replay.local` (or custom hostname) resolves.
- As a fallback, the UI should display the gateway IP (`10.42.0.1`) so users can connect even without mDNS support (some Android versions).

### How Replay Control Remains Accessible

Replay Control binds to `0.0.0.0:8080`, so it listens on all interfaces regardless of WiFi mode. When the Pi switches to hotspot:

1. Client connects to the "ReplayOS" WiFi network
2. Opens `http://replay.local:8080` or `http://10.42.0.1:8080`
3. The app works identically -- all server functions are local

### Edge Cases

- **First boot**: If no WiFi is configured in `replay.cfg`, hotspot could be the default (like WiPi). This provides a zero-config experience for new users.
- **Mode persistence**: The `wifi_hotspot` flag in `replay.cfg` ensures the mode survives reboots. A startup script or systemd unit applies the correct mode on boot.
- **Wired + hotspot**: When ethernet is connected, the Pi can be on the home network (wired) while also running a hotspot. This is the "Hotspot Router" mode from WiPi.

### Complexity: Medium

The `nmcli` command handles the heavy lifting, but the UX around mode switching (warnings, reconnection guidance, fallback IPs) and boot-time mode application need careful design. Testing on real hardware is essential since AP mode behavior varies by Pi model and WiFi chipset.

---

## 3. Recommended Approach

### Phase 1: Hostname Configuration

Implement first -- it's low complexity, self-contained, and immediately useful for multi-Pi setups. Takes the existing WiFi settings page pattern (form + server function + `systemctl` call) and applies it to hostname. Could be added to the existing `/more` page or a new `/more/system` sub-page. Estimated effort: 1-2 sessions.

### Phase 2: WiFi Hotspot Mode

Implement after hostname, since the hostname feature helps with hotspot (users need `<custom-name>.local` to find their Pi on the hotspot network). Steps:

1. Add hotspot toggle + config fields to the WiFi settings page
2. Implement the server function that calls `nmcli device wifi hotspot`
3. Add a boot-time script/service that reads `replay.cfg` and applies the correct WiFi mode
4. Test on real Pi hardware (AP mode support varies)

Estimated effort: 2-3 sessions, plus hardware testing.

### Phase 3 (future): Zero-Config Default

Make hotspot the default when no WiFi client is configured, matching WiPi's approach. This is a UX decision that should be validated with users first.

---

## 4. Hotspot / WiFi Client Coexistence Analysis

This section analyzes how hotspot mode interacts with the existing RePlayOS WiFi client configuration, and how to ensure the two modes do not interfere with each other.

### 4.1 How RePlayOS Manages WiFi Today

Although NetworkManager is installed on RePlayOS (Debian Trixie), **WiFi is managed through `wpa_supplicant`**, not NetworkManager. The official RePlayOS WiFi docs instruct users to edit `/etc/wpa_supplicant/wpa_supplicant-wlan0.conf` directly, and apply changes via `wpa_cli -i wlan0 reconfigure`. The flow is:

1. User writes `wifi_name`, `wifi_pwd`, `wifi_country`, `wifi_mode`, `wifi_hidden` to `replay.cfg`
2. On boot (or on settings apply), RePlayOS reads these values and generates the appropriate `wpa_supplicant` configuration
3. RePlayOS **masks the password** in `replay.cfg` (replacing it with `********`), which serves dual purposes: security, and as a sentinel so RePlayOS knows the config has already been applied
4. The masked password means RePlayOS re-applies WiFi config only when the password is *not* `********` -- i.e., when a new password has been written

This password-masking behavior has important implications for hotspot mode.

### 4.2 Single-Radio Constraint: Can Both Modes Coexist?

**Short answer: No, not on most Raspberry Pi models.**

The Raspberry Pi 3B/3B+, 4B, and 5 all have a single WiFi radio (one `wlan0` interface). A single radio can operate in either **station (client) mode** or **AP (access point/hotspot) mode**, but not both simultaneously -- at least not without a virtual interface (`iw dev wlan0 interface add`) and specific chipset/driver support.

**Concurrent AP+STA on Raspberry Pi:**

- The Broadcom/Cypress chipsets used in Pi 3/4 (BCM43455/CYW43455) technically support multi-role operation through virtual interfaces. However, in practice this is fragile: both interfaces share the same radio and channel, throughput is halved, and some driver/firmware combinations do not support it.
- The Pi 5 uses a different chipset (CYW43455 or similar) but the same constraints generally apply.
- **NetworkManager** can manage concurrent AP+STA via connection profiles, but since RePlayOS uses `wpa_supplicant` directly for client mode, mixing NetworkManager AP mode with wpa_supplicant client mode would create conflicts (both trying to control `wlan0`).

**Recommendation: Treat hotspot and client as mutually exclusive modes**, matching WiPi Netbooter's approach. This is reliable, well-understood, and avoids chipset-specific fragility. If ethernet is connected, it provides home network access while the WiFi radio runs in hotspot mode -- this is the "Hotspot Router" pattern from WiPi.

### 4.3 Impact on replay.cfg WiFi Configuration

When switching between client and hotspot mode, the existing WiFi client settings (`wifi_name`, `wifi_pwd`, etc.) must be preserved. There are two concerns:

**Concern 1: Password masking**

RePlayOS masks `wifi_pwd` to `********` after applying WiFi settings. If switching to hotspot mode triggered a WiFi reconfiguration cycle, it could interfere with the password masking logic. However, since hotspot mode uses **separate config keys** (`wifi_hotspot`, `wifi_hotspot_ssid`, `wifi_hotspot_pwd`, `wifi_hotspot_band` as proposed in Section 2), the client WiFi keys remain untouched. The password-masking sentinel continues to function correctly.

**Concern 2: wpa_supplicant state**

When activating hotspot mode, the `wpa_supplicant` service for `wlan0` must be stopped (since the radio is being repurposed for AP mode). When switching back to client mode, `wpa_supplicant` must be restarted with the existing configuration. The `/etc/wpa_supplicant/wpa_supplicant-wlan0.conf` file should NOT be modified during hotspot activation -- it should simply be left in place so that client mode can resume cleanly.

**Conclusion: The existing WiFi client config in replay.cfg is safe** as long as hotspot mode:
- Uses separate config keys (not `wifi_name`/`wifi_pwd`)
- Does not trigger RePlayOS's WiFi apply logic
- Does not modify or delete the wpa_supplicant configuration file

### 4.4 Mode Switching: What Happens

#### Client -> Hotspot

1. Stop `wpa_supplicant@wlan0.service` (or equivalent)
2. Start the hotspot via `nmcli device wifi hotspot` or by configuring `hostapd` directly
3. The Pi loses its client WiFi connection -- any user connected via the home network loses access
4. The Pi's hotspot network becomes available; users reconnect to it
5. `wifi_hotspot = "true"` is written to `replay.cfg`
6. The existing `wifi_name`, `wifi_pwd`, etc. remain untouched in `replay.cfg`
7. `/etc/wpa_supplicant/wpa_supplicant-wlan0.conf` remains on disk, unchanged

#### Hotspot -> Client

1. Stop the hotspot service (NetworkManager AP connection or hostapd)
2. Restart `wpa_supplicant@wlan0.service` -- it reads the existing config file and connects to the previously configured network
3. The Pi loses its hotspot network -- any device connected to the hotspot loses access
4. The Pi rejoins the home WiFi network
5. `wifi_hotspot = "false"` is written to `replay.cfg`

In both directions, the switch is a **disconnect event** for the current session. The UI must warn the user clearly before performing the switch.

### 4.5 Separate Config Keys (Recommended)

Hotspot configuration should be fully separate from client WiFi configuration:

| Setting | Client Mode Key | Hotspot Mode Key |
|---------|----------------|-----------------|
| Enable | *(implicit when wifi_name is set)* | `wifi_hotspot` |
| Network name | `wifi_name` | `wifi_hotspot_ssid` |
| Password | `wifi_pwd` | `wifi_hotspot_pwd` |
| Country | `wifi_country` | *(shared: same regulatory domain)* |
| Security mode | `wifi_mode` | *(always WPA2, AP mode does not need transition/WPA3 variants)* |
| Hidden network | `wifi_hidden` | *(not applicable: we control the AP)* |
| Band | *(not applicable)* | `wifi_hotspot_band` |

**Why separate keys:**

- **No cross-contamination.** Editing hotspot settings never risks corrupting client WiFi settings and vice versa.
- **Password masking independence.** RePlayOS's password masking on `wifi_pwd` is completely decoupled from hotspot password handling. Replay Control can manage the hotspot password independently (it could also mask it, or handle it differently since the user chose the password rather than entering a pre-existing one).
- **Clean rollback.** If a user enables hotspot mode and later disables it, the original client WiFi config is intact -- no need to re-enter SSID or password.
- **Boot logic clarity.** The startup script reads `wifi_hotspot`: if `"true"`, start AP mode; if `"false"` (or absent), apply the normal client WiFi flow. Simple branching, no ambiguity.

The only shared setting is `wifi_country`, since the regulatory domain applies regardless of whether the radio is in client or AP mode. Using one key for this avoids contradictory country settings.

### 4.6 Safety Considerations

#### Loss of network access

Switching to hotspot mode means the Pi is no longer on the home network. If the user's only way to access Replay Control is via home WiFi, they will lose access until they connect to the hotspot network. Mitigations:

- **Pre-switch warning:** "Switching to hotspot mode will disconnect from your home WiFi. You will need to connect your device to the 'ReplayOS' WiFi network and navigate to http://replay.local:8080 or http://10.42.0.1:8080 to continue."
- **Display the hotspot SSID and password** in the warning dialog so the user can note them down before confirming.
- **Ethernet fallback.** If ethernet is connected, the Pi remains reachable on the home network via its wired IP. The warning should detect this: "Ethernet is connected, so you can also access Replay Control at http://&lt;wired-ip&gt;:8080."
- **Timeout/auto-revert (optional).** Similar to how display settings changes work: "If you don't confirm within 60 seconds, hotspot mode will be reverted." This prevents a user from being permanently locked out if they can't connect to the hotspot for some reason (e.g., the hotspot SSID/password was wrong, or their device doesn't support 5GHz and the hotspot was set to `"a"` band). This adds complexity but is a strong safety net.

#### Boot-time safety

If `wifi_hotspot = "true"` is in `replay.cfg` but the hotspot fails to start (e.g., driver issue, hardware problem), the Pi should fall back to client WiFi mode. The boot script should:

1. Attempt to start hotspot mode
2. Wait a few seconds and verify the AP interface is up
3. If it failed, log the error and fall back to starting `wpa_supplicant` in client mode
4. Optionally reset `wifi_hotspot` to `"false"` so the failure doesn't repeat every boot

#### Password handling for hotspot

Unlike client WiFi where the password is a secret the user enters (someone else's network), the hotspot password is one the user *creates*. Different considerations:

- The hotspot password should still not be sent to the browser unless needed (e.g., for the "show current hotspot password" feature).
- Masking it in `replay.cfg` (like `wifi_pwd`) is an option, but less critical since it's not a credential for an external service.
- If we do mask it, we need another way to know the current hotspot password (since RePlayOS masking replaces the value). One option: store the actual hotspot password in a root-only file outside `replay.cfg`, or use NetworkManager's connection storage (which keeps it in `/etc/NetworkManager/system-connections/`).

#### What if RePlayOS adds its own hotspot support?

RePlayOS could add native hotspot/AP mode support to `replay.cfg` in a future version. Using the `wifi_hotspot_*` key naming convention keeps our config additions consistent with the `wifi_*` namespace. If RePlayOS adds conflicting keys, we would need to adapt -- but this is unlikely to happen without notice since RePlayOS is actively developed and the changelog would reveal it.

### 4.7 Implementation Choice: NetworkManager vs hostapd

The existing document (Section 2) proposes `nmcli device wifi hotspot`, which uses NetworkManager. However, since RePlayOS uses `wpa_supplicant` for client WiFi, there is a potential conflict: NetworkManager and `wpa_supplicant` both trying to manage `wlan0`.

**Option A: NetworkManager for AP mode (with wpa_supplicant stopped)**

- Stop `wpa_supplicant@wlan0` before starting the hotspot
- Use `nmcli device wifi hotspot` to create the AP
- When switching back, stop the NM hotspot and restart `wpa_supplicant@wlan0`
- Pro: Simple one-liner for AP creation, built-in DHCP
- Con: Relies on NetworkManager being configured to manage `wlan0` when wpa_supplicant is stopped; may need `nmcli device set wlan0 managed yes`

**Option B: hostapd + dnsmasq directly**

- Stop `wpa_supplicant@wlan0` before starting the hotspot
- Write a minimal `hostapd.conf` and start `hostapd`
- Run `dnsmasq` for DHCP/DNS on the AP interface
- When switching back, stop hostapd/dnsmasq and restart `wpa_supplicant@wlan0`
- Pro: No dependency on NetworkManager managing WiFi; explicit control
- Con: More configuration files to manage, two additional services

**Recommendation: Option A** (NetworkManager), as proposed in Section 2. It is simpler and avoids managing hostapd/dnsmasq configs. The key is to cleanly stop `wpa_supplicant` before handing `wlan0` to NetworkManager, and to cleanly hand it back when returning to client mode. Testing on real hardware will confirm whether NetworkManager picks up `wlan0` cleanly after `wpa_supplicant` releases it.
