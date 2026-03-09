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
- **Option B: `replay.cfg`** -- add a `system_hostname` key. On boot (or on save), Replay Control applies it via `hostnamectl`. This keeps the setting visible/portable with the SD card, matching how `wifi_name` already lives in `replay.cfg`.

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
