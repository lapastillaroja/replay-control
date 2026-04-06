#!/usr/bin/env bash
set -euo pipefail

# ── Replay Control Installer ──────────────────────────────────────────
#
# Installs the Replay Control on a Raspberry Pi running RePlayOS.
#
# Usage:
#   curl -sSL https://github.com/lapastillaroja/replay-control/releases/latest/download/install.sh | bash -s -- --ip replay.local
#   bash install.sh --sdcard
#   bash install.sh --ip 192.168.1.50
#   bash install.sh --dry-run
#   REPLAY_CONTROL_VERSION=v0.2.0 bash install.sh
#   REPLAY_CONTROL_VERSION=beta bash install.sh

# ── Constants ───────────────────────────────────────────────────────────────

REPO="lapastillaroja/replay-control"
PI_USER="root"
PI_PASSWORD="${PI_PASS:-replayos}"
INSTALL_DIR="/usr/local/bin"
SITE_DIR="/usr/local/share/replay"
SERVICE_NAME="replay-control"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
ENV_FILE="/etc/default/${SERVICE_NAME}"
AVAHI_FILE="/etc/avahi/services/${SERVICE_NAME}.service"
DEFAULT_PORT="8080"

SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10"

# ── Colors ──────────────────────────────────────────────────────────────────

if [[ -t 1 ]] && [[ -n "${TERM:-}" ]] && command -v tput &>/dev/null; then
    RED=$(tput setaf 1)
    GREEN=$(tput setaf 2)
    YELLOW=$(tput setaf 3)
    BLUE=$(tput setaf 4)
    BOLD=$(tput bold)
    RESET=$(tput sgr0)
else
    RED="" GREEN="" YELLOW="" BLUE="" BOLD="" RESET=""
fi

# ── Output helpers ──────────────────────────────────────────────────────────

info()    { echo "${BLUE}::${RESET} $*"; }
success() { echo "${GREEN}✓${RESET} $*"; }
warn()    { echo "${YELLOW}!${RESET} $*"; }
error()   { echo "${RED}✗${RESET} $*" >&2; }
fatal()   { error "$@"; exit 1; }
dry()     { echo "${YELLOW}[DRY RUN]${RESET} $*"; }

# ── Globals ─────────────────────────────────────────────────────────────────

MODE="ssh"           # ssh or sdcard
ACTION="install"     # install or uninstall
DRY_RUN=false
LOCAL=false
LOCAL_DIR=""
PI_ADDR="${REPLAY_PI_ADDR:-}"
VERSION="${REPLAY_CONTROL_VERSION:-latest}"
SDCARD_PATH=""
TMPDIR_WORK=""

# ── Cleanup ─────────────────────────────────────────────────────────────────

cleanup() {
    if [[ -n "${TMPDIR_WORK:-}" ]] && [[ -d "${TMPDIR_WORK}" ]]; then
        rm -rf "$TMPDIR_WORK"
    fi
}
trap cleanup EXIT

# ── Usage ───────────────────────────────────────────────────────────────────

usage() {
    cat <<EOF
${BOLD}Replay Control Installer${RESET}

Installs the Replay Control on a Raspberry Pi running RePlayOS.

${BOLD}USAGE${RESET}
    install.sh [FLAGS]

${BOLD}FLAGS${RESET}
    --help              Show this help message
    --uninstall         Remove the app from a connected Pi via SSH
    --sdcard [PATH]     Write directly to a mounted RePlayOS SD card
    --ip ADDRESS        Skip Pi discovery, use this IP address
    --dry-run           Show what would be done without making changes
    --local [DIR]       Use locally built artifacts instead of downloading
                        (default: project root, expects target/release/ and target/site/)

${BOLD}ENVIRONMENT VARIABLES${RESET}
    REPLAY_CONTROL_VERSION  Release to install: tag, "latest" (default), or "beta"
    REPLAY_PI_ADDR          Pi address, same as --ip

${BOLD}EXAMPLES${RESET}
    ${BOLD}Install latest stable via SSH:${RESET}
        bash install.sh

    ${BOLD}Install latest beta:${RESET}
        REPLAY_CONTROL_VERSION=beta bash install.sh

    ${BOLD}Install via SSH to a known IP:${RESET}
        bash install.sh --ip 192.168.1.50

    ${BOLD}Install to a mounted SD card:${RESET}
        bash install.sh --sdcard /run/media/user/rootfs

    ${BOLD}Install a specific version:${RESET}
        REPLAY_CONTROL_VERSION=v0.2.0 bash install.sh

    ${BOLD}Install from local build:${RESET}
        bash install.sh --local

    ${BOLD}Preview what would happen:${RESET}
        bash install.sh --dry-run

    ${BOLD}Pipe from curl:${RESET}
        curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | bash
EOF
}

# ── Argument parsing ────────────────────────────────────────────────────────

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --help|-h)
                usage
                exit 0
                ;;
            --uninstall)
                ACTION="uninstall"
                shift
                ;;
            --sdcard)
                MODE="sdcard"
                shift
                if [[ $# -gt 0 ]] && [[ "$1" != --* ]]; then
                    SDCARD_PATH="$1"
                    shift
                fi
                ;;
            --ip)
                shift
                [[ $# -eq 0 ]] && fatal "Missing address after --ip"
                PI_ADDR="$1"
                shift
                ;;
            --local)
                LOCAL=true
                shift
                if [[ $# -gt 0 ]] && [[ "$1" != --* ]]; then
                    LOCAL_DIR="$1"
                    shift
                fi
                ;;
            --dry-run)
                DRY_RUN=true
                shift
                ;;
            *)
                fatal "Unknown option: $1 (try --help)"
                ;;
        esac
    done
}

# ── Release URL resolution ─────────────────────────────────────────────────

resolve_download_urls() {
    local base_url

    if [[ "$VERSION" == "beta" ]]; then
        info "Querying GitHub for latest beta release..."
        local tag
        tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases" \
            | grep -o '"tag_name": *"[^"]*"' \
            | head -1 \
            | sed 's/.*"\(v[^"]*\)".*/\1/')
        if [[ -z "$tag" ]]; then
            fatal "No releases found. Check https://github.com/$REPO/releases"
        fi
        info "Found: $tag"
        VERSION="$tag"
        base_url="https://github.com/$REPO/releases/download/$tag"
    elif [[ "$VERSION" == "latest" ]]; then
        base_url="https://github.com/$REPO/releases/latest/download"
    else
        base_url="https://github.com/$REPO/releases/download/$VERSION"
    fi

    BINARY_URL="${base_url}/replay-control-app-aarch64-linux.tar.gz"
    SITE_URL="${base_url}/replay-site.tar.gz"
}

# ── Download / prepare artifacts ───────────────────────────────────────────

prepare_local_artifacts() {
    local project_dir="${LOCAL_DIR:-$(pwd)}"
    local site_dir="$project_dir/target/site"

    # Prefer aarch64 binary (for Pi), fall back to native
    local binary=""
    if [[ -f "$project_dir/target/aarch64-unknown-linux-gnu/release/replay-control-app" ]]; then
        binary="$project_dir/target/aarch64-unknown-linux-gnu/release/replay-control-app"
    elif [[ -f "$project_dir/target/release/replay-control-app" ]]; then
        binary="$project_dir/target/release/replay-control-app"
        local arch
        arch="$(file "$binary" 2>/dev/null || true)"
        if [[ "$arch" == *"x86-64"* ]] && [[ "$MODE" == "ssh" ]]; then
            warn "Using x86_64 binary — this won't run on the Pi. Build with: ./build.sh --target aarch64"
        fi
    fi

    if [[ -z "$binary" ]] || [[ ! -f "$binary" ]]; then
        fatal "Local binary not found at:
  $project_dir/target/aarch64-unknown-linux-gnu/release/replay-control-app
  $project_dir/target/release/replay-control-app
  Run ./build.sh first, or specify the project directory: --local /path/to/replay"
    fi

    if [[ ! -d "$site_dir" ]]; then
        fatal "Local site assets not found: $site_dir
  Run ./build.sh first, or specify the project directory: --local /path/to/replay"
    fi

    TMPDIR_WORK="$(mktemp -d)"

    info "Packaging local build artifacts..."

    if $DRY_RUN; then
        dry "Would package binary from: $binary"
        dry "Would package site assets from: $site_dir"
        dry "Would save to temp directory: $TMPDIR_WORK"
        return
    fi

    # Create the same tar archives the remote flow expects
    tar -czf "$TMPDIR_WORK/replay-control-app-aarch64-linux.tar.gz" -C "$(dirname "$binary")" "$(basename "$binary")"
    tar -czf "$TMPDIR_WORK/replay-site.tar.gz" -C "$(dirname "$site_dir")" "$(basename "$site_dir")"

    success "Packaged local artifacts"
}

download_artifacts() {
    TMPDIR_WORK="$(mktemp -d)"

    info "Downloading Replay Control (version: $VERSION)..."

    if $DRY_RUN; then
        dry "Would download: $BINARY_URL"
        dry "Would download: $SITE_URL"
        dry "Would save to temp directory: $TMPDIR_WORK"
        return
    fi

    if ! curl -fSL --progress-bar -o "$TMPDIR_WORK/replay-control-app-aarch64-linux.tar.gz" "$BINARY_URL"; then
        if [[ "$VERSION" != "latest" ]]; then
            fatal "Release $VERSION not found. Check https://github.com/$REPO/releases for available versions."
        else
            fatal "Cannot download release. Check your internet connection."
        fi
    fi

    if ! curl -fSL --progress-bar -o "$TMPDIR_WORK/replay-site.tar.gz" "$SITE_URL"; then
        fatal "Cannot download site assets. Check your internet connection."
    fi

    success "Downloaded release artifacts"
}

fetch_artifacts() {
    if $LOCAL; then
        prepare_local_artifacts
    else
        resolve_download_urls
        download_artifacts
    fi
}

# ── Pi discovery ────────────────────────────────────────────────────────────

discover_pi() {
    # Already set via --ip or REPLAY_PI_ADDR
    if [[ -n "$PI_ADDR" ]]; then
        info "Using Pi address: $PI_ADDR"
        return
    fi

    # Try mDNS
    info "Looking for your RePlay Pi on the network..."

    # Try known mDNS hostnames (RePlayOS default hostname is "replay")
    local -a mdns_names=("replay.local" "replaypi.local")

    for name in "${mdns_names[@]}"; do
        if command -v getent &>/dev/null && getent hosts "$name" &>/dev/null; then
            PI_ADDR="$name"
            success "Found Pi via mDNS: $PI_ADDR"
            return
        fi

        if command -v avahi-resolve &>/dev/null && avahi-resolve -n "$name" &>/dev/null 2>&1; then
            PI_ADDR="$name"
            success "Found Pi via Avahi: $PI_ADDR"
            return
        fi

        if ping -c 1 -W 2 "$name" &>/dev/null; then
            PI_ADDR="$name"
            success "Found Pi via ping: $PI_ADDR"
            return
        fi
    done

    # Prompt user
    if $DRY_RUN; then
        PI_ADDR="<not-discovered>"
        dry "mDNS discovery failed. Would prompt user for Pi IP address."
        return
    fi

    echo ""
    warn "Could not find your RePlay Pi automatically."
    echo "  Enter its IP address (you can find this in your router's admin page)."
    echo ""
    read -rp "  Pi IP address: " PI_ADDR
    [[ -z "$PI_ADDR" ]] && fatal "No address provided."
}

# ── SSH connectivity check ──────────────────────────────────────────────────

check_ssh_connectivity() {
    if $DRY_RUN; then
        dry "Would check SSH connectivity to $PI_ADDR:22"
        return
    fi

    info "Checking SSH connectivity to $PI_ADDR..."

    # Quick TCP check on port 22
    if command -v nc &>/dev/null; then
        if ! nc -z -w 5 "$PI_ADDR" 22 &>/dev/null; then
            fatal "Cannot connect to ${PI_ADDR}:22. Is the Pi powered on and connected to your network?"
        fi
    elif command -v bash &>/dev/null; then
        if ! (echo >/dev/tcp/"$PI_ADDR"/22) 2>/dev/null; then
            fatal "Cannot connect to ${PI_ADDR}:22. Is the Pi powered on and connected to your network?"
        fi
    fi

    success "SSH port reachable"
}

# ── SSH askpass setup ───────────────────────────────────────────────────────

setup_askpass() {
    ASKPASS_FILE="$(mktemp)"
    printf '#!/bin/sh\necho "%s"\n' "$PI_PASSWORD" > "$ASKPASS_FILE"
    chmod +x "$ASKPASS_FILE"
    export SSH_ASKPASS="$ASKPASS_FILE"
    export SSH_ASKPASS_REQUIRE="force"
    # Unset DISPLAY to avoid X11 askpass dialogs
    export DISPLAY=
}

teardown_askpass() {
    if [[ -n "${ASKPASS_FILE:-}" ]]; then
        rm -f "$ASKPASS_FILE"
        unset SSH_ASKPASS SSH_ASKPASS_REQUIRE
    fi
}

# ── Remote SSH command wrapper ──────────────────────────────────────────────

run_ssh() {
    # shellcheck disable=SC2086
    ssh $SSH_OPTS "${PI_USER}@${PI_ADDR}" "$@"
}

run_scp() {
    # shellcheck disable=SC2086
    scp $SSH_OPTS "$@"
}

# ── Systemd unit contents ──────────────────────────────────────────────────

systemd_service_content() {
    cat <<'UNIT'
[Unit]
Description=Replay Control
After=network.target
After=media-sd.mount media-usb.mount

[Service]
Type=simple
EnvironmentFile=-/etc/default/replay-control
ExecStart=/usr/local/bin/replay-control-app \
    --port ${REPLAY_PORT} \
    --site-root ${REPLAY_SITE_ROOT}
Restart=on-failure
RestartSec=5
StandardOutput=append:/var/log/replay-control.log
StandardError=append:/var/log/replay-control.log
SyslogIdentifier=replay-control

[Install]
WantedBy=multi-user.target
UNIT
}

env_file_content() {
    cat <<'ENV'
# Port for the web UI
REPLAY_PORT=8080

# Path to static site assets
REPLAY_SITE_ROOT=/usr/local/share/replay/site

# Uncomment to override auto-detected storage path
#REPLAY_STORAGE_PATH=/media/sd

# Uncomment to override auto-detected config path
#REPLAY_CONFIG_PATH=/media/sd/config/replay.cfg

# Log level (trace, debug, info, warn, error)
RUST_LOG=replay_control_app=info,replay_control_core=info
ENV
}

avahi_service_content() {
    cat <<'AVAHI'
<?xml version="1.0" standalone='no'?>
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name>Replay Control</name>
  <service>
    <type>_http._tcp</type>
    <port>8080</port>
  </service>
</service-group>
AVAHI
}

# ── SSH install ─────────────────────────────────────────────────────────────

install_ssh() {
    discover_pi
    check_ssh_connectivity
    fetch_artifacts

    if $DRY_RUN; then
        dry "Would set up SSH_ASKPASS for password automation"
        dry "Would transfer replay-control-app-aarch64-linux.tar.gz to ${PI_USER}@${PI_ADDR}:/tmp/"
        dry "Would transfer replay-site.tar.gz to ${PI_USER}@${PI_ADDR}:/tmp/"
        echo ""
        dry "Would run installation commands on Pi via SSH:"
        dry "  - Extract binary to ${INSTALL_DIR}/replay-control-app"
        dry "  - Extract site assets to ${SITE_DIR}/site/"
        dry "  - Write systemd service to ${SERVICE_FILE}"
        dry "  - Write environment file to ${ENV_FILE} (only if not present)"
        dry "  - Write Avahi service to ${AVAHI_FILE} (if Avahi is available)"
        dry "  - Run: systemctl daemon-reload"
        dry "  - Run: systemctl enable ${SERVICE_NAME}"
        dry "  - Run: systemctl restart ${SERVICE_NAME}"
        dry "  - Clean up temp files on Pi"
        echo ""
        dry "Would verify service is running"
        dry "App would be available at: http://${PI_ADDR}:${DEFAULT_PORT}"
        return
    fi

    setup_askpass

    info "Transferring files to Pi..."

    run_scp "$TMPDIR_WORK/replay-control-app-aarch64-linux.tar.gz" "${PI_USER}@${PI_ADDR}:/tmp/" || {
        teardown_askpass
        fatal "Failed to transfer binary archive. SSH authentication may have failed."
    }

    run_scp "$TMPDIR_WORK/replay-site.tar.gz" "${PI_USER}@${PI_ADDR}:/tmp/" || {
        teardown_askpass
        fatal "Failed to transfer site archive."
    }

    success "Files transferred"

    info "Installing on Pi..."

    run_ssh bash -s <<'REMOTE_INSTALL'
set -euo pipefail

# Extract binary
tar -xzf /tmp/replay-control-app-aarch64-linux.tar.gz -C /tmp/
mkdir -p /usr/local/bin
install -m755 /tmp/replay-control-app /usr/local/bin/replay-control-app

# Extract site assets
rm -rf /usr/local/share/replay/site
mkdir -p /usr/local/share/replay
tar -xzf /tmp/replay-site.tar.gz -C /usr/local/share/replay/

# Write systemd service file
cat > /etc/systemd/system/replay-control.service << 'UNIT'
[Unit]
Description=Replay Control
After=network.target
After=media-sd.mount media-usb.mount

[Service]
Type=simple
EnvironmentFile=-/etc/default/replay-control
ExecStart=/usr/local/bin/replay-control-app \
    --port ${REPLAY_PORT} \
    --site-root ${REPLAY_SITE_ROOT}
Restart=on-failure
RestartSec=5
StandardOutput=append:/var/log/replay-control.log
StandardError=append:/var/log/replay-control.log
SyslogIdentifier=replay-control

[Install]
WantedBy=multi-user.target
UNIT

# Write default environment file (preserve existing)
if [ ! -f /etc/default/replay-control ]; then
    cat > /etc/default/replay-control << 'ENV'
# Port for the web UI
REPLAY_PORT=8080

# Path to static site assets
REPLAY_SITE_ROOT=/usr/local/share/replay/site

# Uncomment to override auto-detected storage path
#REPLAY_STORAGE_PATH=/media/sd

# Uncomment to override auto-detected config path
#REPLAY_CONFIG_PATH=/media/sd/config/replay.cfg

# Log level (trace, debug, info, warn, error)
RUST_LOG=replay_control_app=info,replay_control_core=info
ENV
fi

# Enable Avahi for mDNS discovery (.local hostname)
if command -v avahi-daemon &>/dev/null; then
    systemctl enable avahi-daemon 2>/dev/null || true
    systemctl start avahi-daemon 2>/dev/null || true

    if [ -d /etc/avahi/services ]; then
        cat > /etc/avahi/services/replay-control.service << 'AVAHI'
<?xml version="1.0" standalone='no'?>
<!DOCTYPE service-group SYSTEM "avahi-service.dtd">
<service-group>
  <name>Replay Control</name>
  <service>
    <type>_http._tcp</type>
    <port>8080</port>
  </service>
</service-group>
AVAHI
    fi
fi

# Reload and start
systemctl daemon-reload
systemctl enable replay-control
systemctl restart replay-control

# Cleanup
rm -f /tmp/replay-control-app-aarch64-linux.tar.gz /tmp/replay-site.tar.gz /tmp/replay-control-app
REMOTE_INSTALL

    teardown_askpass

    success "Installation complete"

    # Verify service
    info "Verifying service..."
    setup_askpass
    if run_ssh systemctl is-active "$SERVICE_NAME" &>/dev/null; then
        success "Service is running"
    else
        warn "Service may not have started yet. Check with: ssh ${PI_USER}@${PI_ADDR} systemctl status ${SERVICE_NAME}"
    fi
    teardown_askpass

    echo ""
    success "${BOLD}Replay Control installed!${RESET}"
    echo "  Open ${GREEN}http://${PI_ADDR}:${DEFAULT_PORT}${RESET} in your browser."
    echo ""
}

# ── SSH uninstall ───────────────────────────────────────────────────────────

uninstall_ssh() {
    discover_pi
    check_ssh_connectivity

    if $DRY_RUN; then
        dry "Would set up SSH_ASKPASS for password automation"
        dry "Would run uninstall commands on Pi via SSH:"
        dry "  - Run: systemctl stop ${SERVICE_NAME}"
        dry "  - Run: systemctl disable ${SERVICE_NAME}"
        dry "  - Remove: ${SERVICE_FILE}"
        dry "  - Remove: ${AVAHI_FILE}"
        dry "  - Remove: ${INSTALL_DIR}/replay-control-app"
        dry "  - Remove: ${SITE_DIR}/"
        dry "  - Run: systemctl daemon-reload"
        dry "  Note: ${ENV_FILE} would be preserved"
        return
    fi

    setup_askpass

    info "Uninstalling from Pi..."

    run_ssh bash -s <<'REMOTE_UNINSTALL'
set -euo pipefail

systemctl stop replay-control 2>/dev/null || true
systemctl disable replay-control 2>/dev/null || true
rm -f /etc/systemd/system/replay-control.service
rm -f /etc/avahi/services/replay-control.service
rm -f /usr/local/bin/replay-control-app
rm -rf /usr/local/share/replay
systemctl daemon-reload

echo "Note: /etc/default/replay-control was preserved (remove manually if desired)"
REMOTE_UNINSTALL

    teardown_askpass

    success "Replay Control uninstalled from Pi"
}

# ── SD card detection ───────────────────────────────────────────────────────
#
# RePlayOS SD cards have a recognizable partition layout:
#   - bootfs (vfat)  -- contains issue.txt with "RePlay OS"
#   - rootfs (ext4)  -- root filesystem (where we install to)
#   - replay (exfat) -- data partition with roms/, bios/, config/replay.cfg, saves/, captures/
#
# Detection strategy:
#   1. Look for mounted partitions whose sibling partitions indicate RePlayOS
#   2. Check for the "replay" data partition with characteristic directories
#   3. Check for "bootfs" partition with issue.txt containing "RePlay OS"
#   4. The rootfs partition is where we need to write system files
#

is_replayos_data_partition() {
    local path="$1"
    # Check for the characteristic RePlayOS data partition structure
    [[ -d "$path/roms" ]] && \
    [[ -d "$path/bios" ]] && \
    [[ -d "$path/config" ]] && \
    [[ -d "$path/saves" ]] && \
    [[ -d "$path/captures" ]] && \
    [[ -f "$path/config/replay.cfg" ]]
}

is_replayos_boot_partition() {
    local path="$1"
    [[ -f "$path/issue.txt" ]] && grep -qi "replay.os" "$path/issue.txt" 2>/dev/null
}

is_replayos_root_partition() {
    local path="$1"
    # A rootfs will have standard Linux directories
    [[ -d "$path/etc" ]] && \
    [[ -d "$path/usr" ]] && \
    [[ -d "$path/bin" ]]
}

# Globals set by find_sdcard_candidates
SDCARD_CANDIDATES=()
REPLAYOS_DETECTED=false
REPLAYOS_DATA_PATH=""
REPLAYOS_BOOT_PATH=""

find_sdcard_candidates() {
    SDCARD_CANDIDATES=()
    REPLAYOS_DETECTED=false
    REPLAYOS_DATA_PATH=""
    REPLAYOS_BOOT_PATH=""

    local -a search_dirs=()

    # Build search paths
    if [[ -n "${USER:-}" ]]; then
        [[ -d "/run/media/$USER" ]] && search_dirs+=("/run/media/$USER"/*)
        [[ -d "/media/$USER" ]] && search_dirs+=("/media/$USER"/*)
    fi
    search_dirs+=(/mnt/*)

    for mount_path in "${search_dirs[@]}"; do
        [[ -d "$mount_path" ]] || continue

        # Remember if we see RePlayOS markers
        if is_replayos_data_partition "$mount_path"; then
            REPLAYOS_DETECTED=true
            REPLAYOS_DATA_PATH="$mount_path"
        fi
        if is_replayos_boot_partition "$mount_path"; then
            REPLAYOS_DETECTED=true
            REPLAYOS_BOOT_PATH="$mount_path"
        fi

        # Strategy 1: This is a rootfs partition itself and a sibling data partition confirms RePlayOS
        if is_replayos_root_partition "$mount_path"; then
            local parent
            parent="$(dirname "$mount_path")"
            for sibling in "$parent"/*; do
                [[ "$sibling" == "$mount_path" ]] && continue
                if is_replayos_data_partition "$sibling" || is_replayos_boot_partition "$sibling"; then
                    SDCARD_CANDIDATES+=("$mount_path")
                    break
                fi
            done
        fi

        # Strategy 2: This is the data partition -- look for a rootfs sibling
        if is_replayos_data_partition "$mount_path"; then
            local parent
            parent="$(dirname "$mount_path")"
            for sibling in "$parent"/*; do
                [[ "$sibling" == "$mount_path" ]] && continue
                if is_replayos_root_partition "$sibling"; then
                    local already_found=false
                    for c in "${SDCARD_CANDIDATES[@]+"${SDCARD_CANDIDATES[@]}"}"; do
                        [[ "$c" == "$sibling" ]] && already_found=true
                    done
                    $already_found || SDCARD_CANDIDATES+=("$sibling")
                    break
                fi
            done
        fi

        # Strategy 3: This is the boot partition -- look for a rootfs sibling
        if is_replayos_boot_partition "$mount_path"; then
            local parent
            parent="$(dirname "$mount_path")"
            for sibling in "$parent"/*; do
                [[ "$sibling" == "$mount_path" ]] && continue
                if is_replayos_root_partition "$sibling"; then
                    local already_found=false
                    for c in "${SDCARD_CANDIDATES[@]+"${SDCARD_CANDIDATES[@]}"}"; do
                        [[ "$c" == "$sibling" ]] && already_found=true
                    done
                    $already_found || SDCARD_CANDIDATES+=("$sibling")
                    break
                fi
            done
        fi
    done

    # Deduplicate in place
    local -a unique=()
    for c in "${SDCARD_CANDIDATES[@]+"${SDCARD_CANDIDATES[@]}"}"; do
        local dup=false
        for u in "${unique[@]+"${unique[@]}"}"; do
            [[ "$c" == "$u" ]] && dup=true
        done
        $dup || unique+=("$c")
    done
    SDCARD_CANDIDATES=("${unique[@]+"${unique[@]}"}")
}

detect_sdcard() {
    # Explicit path provided
    if [[ -n "$SDCARD_PATH" ]]; then
        if [[ ! -d "$SDCARD_PATH" ]]; then
            fatal "SD card path does not exist: $SDCARD_PATH"
        fi
        # Accept it if it looks like a rootfs, or if it's a data partition (warn)
        if is_replayos_root_partition "$SDCARD_PATH"; then
            info "Using SD card root filesystem: $SDCARD_PATH"
            return
        fi
        if is_replayos_data_partition "$SDCARD_PATH"; then
            fatal "That path looks like the RePlayOS data partition, not the root filesystem. The rootfs partition (labeled 'rootfs') is needed for installation."
        fi
        warn "Path does not look like a standard rootfs, but proceeding as requested: $SDCARD_PATH"
        return
    fi

    info "Searching for RePlayOS SD card..."

    find_sdcard_candidates

    if [[ ${#SDCARD_CANDIDATES[@]} -eq 0 ]]; then
        echo ""
        if $REPLAYOS_DETECTED; then
            # We found the SD card but rootfs isn't mounted
            local hint=""
            [[ -n "$REPLAYOS_DATA_PATH" ]] && hint="  Found RePlayOS data partition at: $REPLAYOS_DATA_PATH"
            [[ -n "$REPLAYOS_BOOT_PATH" ]] && hint="${hint:+$hint
}  Found RePlayOS boot partition at: $REPLAYOS_BOOT_PATH"
            fatal "RePlayOS SD card detected, but the rootfs partition is not mounted.

$hint

  The rootfs partition (ext4, usually labeled 'rootfs') must be mounted for
  direct SD card installation. Mount it and try again:

    sudo mount /dev/sdX2 /mnt/replayos-rootfs
    bash install.sh --sdcard /mnt/replayos-rootfs

  Tip: run 'lsblk -o NAME,LABEL,FSTYPE' to find the right partition."
        else
            fatal "No RePlayOS SD card found.

  Mount the SD card and try again, or specify the path explicitly:
    install.sh --sdcard /path/to/rootfs

  The installer needs the rootfs partition (ext4, usually labeled 'rootfs').
  On Linux, you may need to mount it manually:
    sudo mount /dev/sdX2 /mnt/replayos-rootfs"
        fi
    fi

    if [[ ${#SDCARD_CANDIDATES[@]} -eq 1 ]]; then
        SDCARD_PATH="${SDCARD_CANDIDATES[0]}"
        success "Found RePlayOS SD card: $SDCARD_PATH"
        return
    fi

    # Multiple candidates
    echo ""
    info "Multiple RePlayOS SD cards found:"
    local i=1
    for c in "${SDCARD_CANDIDATES[@]}"; do
        echo "  $i) $c"
        ((i++))
    done
    echo ""

    if $DRY_RUN; then
        SDCARD_PATH="${SDCARD_CANDIDATES[0]}"
        dry "Would prompt user to pick. Using first candidate: $SDCARD_PATH"
        return
    fi

    read -rp "  Pick a number [1-${#SDCARD_CANDIDATES[@]}]: " pick
    if [[ "$pick" =~ ^[0-9]+$ ]] && (( pick >= 1 && pick <= ${#SDCARD_CANDIDATES[@]} )); then
        SDCARD_PATH="${SDCARD_CANDIDATES[$((pick-1))]}"
    else
        fatal "Invalid selection."
    fi
}

# ── SD card install ─────────────────────────────────────────────────────────

install_sdcard() {
    detect_sdcard
    fetch_artifacts

    local sd="$SDCARD_PATH"

    if $DRY_RUN; then
        echo ""
        dry "Would extract and install binary:"
        dry "  install -m755 replay-control-app -> ${sd}${INSTALL_DIR}/replay-control-app"
        echo ""
        dry "Would extract and install site assets:"
        dry "  mkdir -p ${sd}${SITE_DIR}"
        dry "  site/ -> ${sd}${SITE_DIR}/site/"
        echo ""
        dry "Would write systemd service:"
        dry "  -> ${sd}${SERVICE_FILE}"
        echo ""
        dry "Would write environment file (only if not present):"
        dry "  -> ${sd}${ENV_FILE}"
        echo ""
        dry "Would enable service for first boot:"
        dry "  ln -sf ${SERVICE_FILE} -> ${sd}/etc/systemd/system/multi-user.target.wants/${SERVICE_NAME}.service"
        echo ""
        dry "Would write Avahi service (if /etc/avahi/services exists):"
        dry "  -> ${sd}${AVAHI_FILE}"
        echo ""
        dry "App would start automatically on next boot at http://replaypi.local:${DEFAULT_PORT}"
        return
    fi

    info "Installing to SD card at $sd..."

    # Extract binary
    tar -xzf "$TMPDIR_WORK/replay-control-app-aarch64-linux.tar.gz" -C "$TMPDIR_WORK/"
    mkdir -p "${sd}${INSTALL_DIR}"
    install -m755 "$TMPDIR_WORK/replay-control-app" "${sd}${INSTALL_DIR}/replay-control-app"
    success "Installed binary"

    # Extract site assets
    rm -rf "${sd}${SITE_DIR}/site"
    mkdir -p "${sd}${SITE_DIR}"
    tar -xzf "$TMPDIR_WORK/replay-site.tar.gz" -C "${sd}${SITE_DIR}/"
    success "Installed site assets"

    # Write systemd service
    mkdir -p "${sd}/etc/systemd/system"
    systemd_service_content > "${sd}${SERVICE_FILE}"
    success "Wrote systemd service"

    # Write environment file (preserve existing)
    mkdir -p "${sd}/etc/default"
    if [[ ! -f "${sd}${ENV_FILE}" ]]; then
        env_file_content > "${sd}${ENV_FILE}"
        success "Wrote environment file"
    else
        info "Environment file already exists, preserving"
    fi

    # Enable service for first boot
    mkdir -p "${sd}/etc/systemd/system/multi-user.target.wants"
    ln -sf "${SERVICE_FILE}" "${sd}/etc/systemd/system/multi-user.target.wants/${SERVICE_NAME}.service"
    success "Enabled service for first boot"

    # Write Avahi service
    if [[ -d "${sd}/etc/avahi/services" ]]; then
        avahi_service_content > "${sd}${AVAHI_FILE}"
        success "Wrote Avahi service"
    fi

    echo ""
    success "${BOLD}Replay Control installed to SD card!${RESET}"
    echo "  Insert the SD card into your Pi and boot it."
    echo "  The app will start automatically at ${GREEN}http://replaypi.local:${DEFAULT_PORT}${RESET}"
    echo ""
}

# ── Main ────────────────────────────────────────────────────────────────────

main() {
    parse_args "$@"

    echo ""
    echo "${BOLD}Replay Control Installer${RESET}"
    if $DRY_RUN; then
        echo "${YELLOW}(dry run -- no changes will be made)${RESET}"
    fi
    echo ""

    case "${ACTION}-${MODE}" in
        install-ssh)
            install_ssh
            ;;
        install-sdcard)
            install_sdcard
            ;;
        uninstall-ssh)
            uninstall_ssh
            ;;
        uninstall-sdcard)
            fatal "--uninstall is only supported via SSH, not SD card mode."
            ;;
    esac
}

main "$@"
