#!/usr/bin/env bash
set -euo pipefail

# ── Replay Control Installer ──────────────────────────────────────────
#
# Installs the Replay Control on a Raspberry Pi running RePlayOS.
#
# Usage:
#   curl -sSL https://github.com/lapastillaroja/replay-control/releases/latest/download/install.sh | bash -s -- --ip replay.local
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
DEFAULT_HTTPS_PORT="8443"

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

# ── Confirmation prompt (destructive actions) ──────────────────────────────

# Reads a yes/no from /dev/tty so it works even when the script is piped
# (e.g. `curl ... | bash -s -- --purge`). Returns 0 on yes, 1 otherwise.
confirm_destructive() {
    local prompt="$1"
    if $ASSUME_YES; then
        return 0
    fi
    if [[ ! -r /dev/tty ]]; then
        error "$prompt"
        error "No TTY available — re-run with --yes to confirm non-interactively."
        return 1
    fi
    local reply=""
    printf '%s%s%s [y/N] ' "${YELLOW}" "$prompt" "${RESET}" > /dev/tty
    IFS= read -r reply < /dev/tty || reply=""
    [[ "$reply" =~ ^[Yy]([Ee][Ss])?$ ]]
}

# ── Globals ─────────────────────────────────────────────────────────────────

MODE="ssh"           # ssh or local
ACTION="install"     # install or uninstall
DRY_RUN=false
PURGE_DATA=false     # uninstall additionally wipes .replay-control/ and the env file
ASSUME_YES=false     # skip the destructive-action confirmation prompt
LOCAL=false
LOCAL_DIR=""
PI_ADDR="${REPLAY_PI_ADDR:-}"
VERSION="${REPLAY_CONTROL_VERSION:-latest}"
TMPDIR_WORK=""

# Candidate storage roots that may hold the .replay-control/ data dir.
# Mirrors replay-control-core-server/src/platform/storage.rs.
REPLAY_STORAGE_ROOTS=(/media/usb /media/nvme /media/sd /media/nfs)

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
When run directly on a RePlayOS Pi, installs locally without SSH.

${BOLD}USAGE${RESET}
    install.sh [FLAGS]

${BOLD}FLAGS${RESET}
    --help              Show this help message
    --uninstall         Remove the app from a connected Pi via SSH
                        (preserves user data on storage, /var/lib/replay-control,
                        and the env file)
    --purge             Like --uninstall but also wipes ALL Replay Control
                        data: /var/lib/replay-control/ (per-storage library
                        DBs + host-global external_metadata.db + cache),
                        .replay-control/ on storage (user_data.db, downloaded
                        media, storage-id), and the env file. ROMs, saves,
                        captures, and BIOS are NOT touched.
    --yes               Skip the confirmation prompt for --purge
    --ip ADDRESS        Skip Pi discovery, use this IP address
    --pi-pass PASSWORD  SSH password for the Pi (default: "replayos")
    --version VERSION   Version to install: tag (v0.2.0), "latest", or "beta"
    --dry-run           Show what would be done without making changes
    --local [DIR]       Use locally built artifacts instead of downloading
                        (default: project root, expects target/release/ and target/site/)

${BOLD}ENVIRONMENT VARIABLES${RESET}
    REPLAY_CONTROL_VERSION  Release to install: tag, "latest" (default), or "beta"
    REPLAY_PI_ADDR          Pi address, same as --ip
    PI_PASS                 SSH password, same as --pi-pass

${BOLD}EXAMPLES${RESET}
    ${BOLD}Install latest stable via SSH:${RESET}
        bash install.sh

    ${BOLD}Install latest beta:${RESET}
        REPLAY_CONTROL_VERSION=beta bash install.sh

    ${BOLD}Install via SSH to a known IP:${RESET}
        bash install.sh --ip 192.168.1.50

    ${BOLD}Install a specific version:${RESET}
        REPLAY_CONTROL_VERSION=v0.2.0 bash install.sh

    ${BOLD}Install from local build:${RESET}
        bash install.sh --local

    ${BOLD}Preview what would happen:${RESET}
        bash install.sh --dry-run

    ${BOLD}Pipe from curl (auto-detects if running on Pi):${RESET}
        curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | bash

    ${BOLD}Wipe everything (destructive — prompts for confirmation):${RESET}
        bash install.sh --purge

    ${BOLD}Wipe everything non-interactively (e.g. when piped from curl):${RESET}
        curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | bash -s -- --purge --yes
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
            --purge)
                ACTION="uninstall"
                PURGE_DATA=true
                shift
                ;;
            --yes|-y)
                ASSUME_YES=true
                shift
                ;;
            --ip)
                shift
                [[ $# -eq 0 ]] && fatal "Missing address after --ip"
                PI_ADDR="$1"
                shift
                ;;
            --pi-pass)
                shift
                [[ $# -eq 0 ]] && fatal "Missing password after --pi-pass"
                PI_PASSWORD="$1"
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
            --version)
                shift
                [[ $# -eq 0 ]] && fatal "Missing version after --version"
                VERSION="$1"
                shift
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
        # Check if a stable (non-prerelease) release exists via the API.
        local stable_tag
        stable_tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
            | grep -o '"tag_name": *"[^"]*"' \
            | sed 's/.*"\(v[^"]*\)".*/\1/' || true)
        if [[ -n "$stable_tag" ]]; then
            info "Found stable release: $stable_tag"
            VERSION="$stable_tag"
            base_url="https://github.com/$REPO/releases/download/$stable_tag"
        else
            warn "No stable release found — falling back to latest beta"
            VERSION="beta"
            resolve_download_urls
            return
        fi
    else
        base_url="https://github.com/$REPO/releases/download/$VERSION"
    fi

    BINARY_URL="${base_url}/replay-control-app-aarch64-linux.tar.gz"
    SITE_URL="${base_url}/replay-site.tar.gz"
    CATALOG_URL="${base_url}/replay-catalog.tar.gz"
}

# ── Download / prepare artifacts ───────────────────────────────────────────

prepare_local_artifacts() {
    local project_dir="${LOCAL_DIR:-$(pwd)}"
    # Respect CARGO_TARGET_DIR (absolute or relative to project_dir), the same
    # way build.sh does. Falls back to the in-tree `target/` directory.
    local target_dir
    if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
        if [[ "$CARGO_TARGET_DIR" = /* ]]; then
            target_dir="$CARGO_TARGET_DIR"
        else
            target_dir="$project_dir/$CARGO_TARGET_DIR"
        fi
    else
        target_dir="$project_dir/target"
    fi
    local site_dir="$target_dir/site"

    # Prefer aarch64 binary (for Pi), fall back to native
    local binary=""
    if [[ -f "$target_dir/aarch64-unknown-linux-gnu/release/replay-control-app" ]]; then
        binary="$target_dir/aarch64-unknown-linux-gnu/release/replay-control-app"
    elif [[ -f "$target_dir/release/replay-control-app" ]]; then
        binary="$target_dir/release/replay-control-app"
        local arch
        arch="$(file "$binary" 2>/dev/null || true)"
        if [[ "$arch" == *"x86-64"* ]] && [[ "$MODE" == "ssh" ]]; then
            warn "Using x86_64 binary — this won't run on the Pi. Build with: ./build.sh --target aarch64"
        fi
    fi

    if [[ -z "$binary" ]] || [[ ! -f "$binary" ]]; then
        fatal "Local binary not found at:
  $target_dir/aarch64-unknown-linux-gnu/release/replay-control-app
  $target_dir/release/replay-control-app
  Run ./build.sh first, or specify the project directory: --local /path/to/replay"
    fi

    if [[ ! -d "$site_dir" ]]; then
        fatal "Local site assets not found: $site_dir
  Run ./build.sh first, or specify the project directory: --local /path/to/replay"
    fi

    # The catalog lives at the project root in dev (mirrors dev.sh / build.sh).
    local catalog="$project_dir/catalog.sqlite"
    if [[ ! -s "$catalog" ]]; then
        fatal "Local catalog not found or empty: $catalog
  Run: cargo run -p build-catalog -- --output catalog.sqlite
  Or run ./build.sh, which builds it as part of the release flow."
    fi

    TMPDIR_WORK="$(mktemp -d)"

    info "Packaging local build artifacts..."

    if $DRY_RUN; then
        dry "Would package binary from: $binary"
        dry "Would package site assets from: $site_dir"
        dry "Would package catalog from: $catalog"
        dry "Would save to temp directory: $TMPDIR_WORK"
        return
    fi

    # Create the same tar archives the remote flow expects
    tar -czf "$TMPDIR_WORK/replay-control-app-aarch64-linux.tar.gz" -C "$(dirname "$binary")" "$(basename "$binary")"
    tar -czf "$TMPDIR_WORK/replay-site.tar.gz" -C "$(dirname "$site_dir")" "$(basename "$site_dir")"
    tar -czf "$TMPDIR_WORK/replay-catalog.tar.gz" -C "$(dirname "$catalog")" "$(basename "$catalog")"

    success "Packaged local artifacts"
}

download_artifacts() {
    TMPDIR_WORK="$(mktemp -d)"

    info "Downloading Replay Control (version: $VERSION)..."

    if $DRY_RUN; then
        dry "Would download: $BINARY_URL"
        dry "Would download: $SITE_URL"
        dry "Would download: $CATALOG_URL"
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

    # The catalog is required at startup (init_catalog opens catalog.sqlite
    # next to the binary). Older betas (< 0.4.0-beta.5) didn't ship it as a
    # release asset; warn rather than abort so users on those versions can
    # still get the binary in place and add the catalog manually.
    if ! curl -fSL --progress-bar -o "$TMPDIR_WORK/replay-catalog.tar.gz" "$CATALOG_URL"; then
        warn "Could not download catalog from $CATALOG_URL"
        warn "The service will not start without catalog.sqlite next to the binary."
        warn "If this release predates the catalog asset, build it locally and copy it to ${INSTALL_DIR}/."
        rm -f "$TMPDIR_WORK/replay-catalog.tar.gz"
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

# ── Local Pi detection ──────────────────────────────────────────────────────

is_running_on_pi() {
    [[ -f /media/sd/config/replay.cfg ]]
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
    if command -v sshpass &>/dev/null; then
        USE_SSHPASS=true
        return
    fi

    USE_SSHPASS=false
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
    if ${USE_SSHPASS:-false}; then
        sshpass -p "$PI_PASSWORD" ssh $SSH_OPTS "${PI_USER}@${PI_ADDR}" "$@"
    else
        ssh $SSH_OPTS "${PI_USER}@${PI_ADDR}" "$@"
    fi
}

run_scp() {
    # shellcheck disable=SC2086
    if ${USE_SSHPASS:-false}; then
        sshpass -p "$PI_PASSWORD" scp $SSH_OPTS "$@"
    else
        scp $SSH_OPTS "$@"
    fi
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
Environment=REPLAY_PORT=8080
Environment=REPLAY_HTTPS_PORT=8443
Environment=REPLAY_EXTRA_ARGS=
EnvironmentFile=-/etc/default/replay-control
ExecStart=/usr/local/bin/replay-control-app \
    --port ${REPLAY_PORT} \
    --https-port ${REPLAY_HTTPS_PORT} \
    --site-root ${REPLAY_SITE_ROOT} \
    $REPLAY_EXTRA_ARGS
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

# HTTPS port for the main app
REPLAY_HTTPS_PORT=8443

# Path to static site assets
REPLAY_SITE_ROOT=/usr/local/share/replay/site

# Extra CLI args. Dangerous debug flags must be explicit here, for example:
#REPLAY_EXTRA_ARGS=--dangerous-disable-https --dangerous-allow-insecure-auth-over-http
REPLAY_EXTRA_ARGS=

# Uncomment to override auto-detected storage path
#REPLAY_STORAGE_PATH=/media/sd

# Uncomment to override auto-detected config path
#REPLAY_CONFIG_PATH=/media/sd/config/replay.cfg

# Advanced: hash-identification workers for library rebuilds/rescans.
# Default is 2 workers for every storage class. Valid range: 1-4.
#REPLAY_CONTROL_IDENTITY_WORKERS=2

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
    <type>_https._tcp</type>
    <port>8443</port>
  </service>
</service-group>
AVAHI
}

# ── Local install (running directly on the Pi) ─────────────────────────────

install_local() {
    fetch_artifacts

    if $DRY_RUN; then
        dry "Would extract binary to ${INSTALL_DIR}/replay-control-app"
        dry "Would extract catalog to ${INSTALL_DIR}/catalog.sqlite"
        dry "Would extract site assets to ${SITE_DIR}/site/"
        dry "Would write systemd service to ${SERVICE_FILE}"
        dry "Would write environment file to ${ENV_FILE} (only if not present)"
        dry "Would write Avahi service to ${AVAHI_FILE} (if Avahi is available)"
        dry "Would run: systemctl daemon-reload && systemctl enable && systemctl restart"
        return
    fi

    info "Installing locally..."

    # Create settings directory (Pi-level settings live here after migration)
    mkdir -p /etc/replay-control
    # Create central data directory for per-storage library DBs.
    # Each ROM storage gets a `storages/<storage_id>/library.db` once attached.
    mkdir -p /var/lib/replay-control/storages

    # Extract binary
    tar -xzf "$TMPDIR_WORK/replay-control-app-aarch64-linux.tar.gz" -C /tmp/
    mkdir -p "$INSTALL_DIR"
    install -m755 /tmp/replay-control-app "$INSTALL_DIR/replay-control-app"

    # Extract catalog next to the binary so resolve_catalog_path picks it up
    # without needing --catalog-path. Required for the service to start.
    if [[ -s "$TMPDIR_WORK/replay-catalog.tar.gz" ]]; then
        tar -xzf "$TMPDIR_WORK/replay-catalog.tar.gz" -C /tmp/
        install -m644 /tmp/catalog.sqlite "$INSTALL_DIR/catalog.sqlite"
    else
        warn "No catalog tarball found — service will fail to start without ${INSTALL_DIR}/catalog.sqlite"
    fi

    # Extract site assets
    rm -rf "$SITE_DIR/site"
    mkdir -p "$SITE_DIR"
    tar -xzf "$TMPDIR_WORK/replay-site.tar.gz" -C "$SITE_DIR/"

    # Write systemd service + env + avahi (reuse shared helpers)
    systemd_service_content > "$SERVICE_FILE"
    if [[ ! -f "$ENV_FILE" ]]; then
        env_file_content > "$ENV_FILE"
    fi
    if command -v avahi-daemon &>/dev/null; then
        systemctl enable avahi-daemon 2>/dev/null || true
        systemctl start avahi-daemon 2>/dev/null || true
        [[ -d /etc/avahi/services ]] && avahi_service_content > "$AVAHI_FILE"
    fi

    # Reload and start
    systemctl daemon-reload
    systemctl enable "$SERVICE_NAME"
    systemctl restart "$SERVICE_NAME"

    # Cleanup
    rm -f /tmp/replay-control-app-aarch64-linux.tar.gz /tmp/replay-site.tar.gz /tmp/replay-catalog.tar.gz /tmp/replay-control-app /tmp/catalog.sqlite

    success "Installation complete"

    # Verify
    info "Verifying service..."
    if systemctl is-active "$SERVICE_NAME" &>/dev/null; then
        success "Service is running"
    else
        warn "Service may not have started yet. Check with: systemctl status ${SERVICE_NAME}"
    fi

    echo ""
    success "${BOLD}Replay Control installed!${RESET}"
    echo "  Open ${GREEN}https://$(hostname).local:${DEFAULT_HTTPS_PORT}${RESET} in your browser."
    echo ""
}

# ── SSH install ────────────────────────────────────────────────────────────

install_ssh() {
    discover_pi
    check_ssh_connectivity
    setup_askpass
    fetch_artifacts

    if $DRY_RUN; then
        dry "Would set up SSH_ASKPASS for password automation"
        dry "Would transfer replay-control-app-aarch64-linux.tar.gz to ${PI_USER}@${PI_ADDR}:/tmp/"
        dry "Would transfer replay-site.tar.gz to ${PI_USER}@${PI_ADDR}:/tmp/"
        dry "Would transfer replay-catalog.tar.gz to ${PI_USER}@${PI_ADDR}:/tmp/"
        echo ""
        dry "Would run installation commands on Pi via SSH:"
        dry "  - Extract binary to ${INSTALL_DIR}/replay-control-app"
        dry "  - Extract catalog to ${INSTALL_DIR}/catalog.sqlite"
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
        dry "App would be available at: https://${PI_ADDR}:${DEFAULT_HTTPS_PORT}"
        return
    fi

    info "Transferring files to Pi..."

    run_scp "$TMPDIR_WORK/replay-control-app-aarch64-linux.tar.gz" "${PI_USER}@${PI_ADDR}:/tmp/" || {
        teardown_askpass
        fatal "Failed to transfer binary archive. SSH authentication may have failed."
    }

    run_scp "$TMPDIR_WORK/replay-site.tar.gz" "${PI_USER}@${PI_ADDR}:/tmp/" || {
        teardown_askpass
        fatal "Failed to transfer site archive."
    }

    if [[ -s "$TMPDIR_WORK/replay-catalog.tar.gz" ]]; then
        run_scp "$TMPDIR_WORK/replay-catalog.tar.gz" "${PI_USER}@${PI_ADDR}:/tmp/" || {
            teardown_askpass
            fatal "Failed to transfer catalog archive."
        }
    else
        warn "No catalog tarball to transfer — service will fail to start without ${INSTALL_DIR}/catalog.sqlite"
    fi

    success "Files transferred"

    info "Installing on Pi..."

    run_ssh bash -s <<'REMOTE_INSTALL'
set -euo pipefail

# Create settings directory (Pi-level settings live here after migration)
mkdir -p /etc/replay-control
# Create central data directory for per-storage library DBs.
# Each ROM storage gets a `storages/<storage_id>/library.db` once attached.
mkdir -p /var/lib/replay-control/storages

# Extract binary
tar -xzf /tmp/replay-control-app-aarch64-linux.tar.gz -C /tmp/
mkdir -p /usr/local/bin
install -m755 /tmp/replay-control-app /usr/local/bin/replay-control-app

# Extract catalog next to the binary so resolve_catalog_path picks it up
# without --catalog-path. Required for the service to start.
if [ -s /tmp/replay-catalog.tar.gz ]; then
    tar -xzf /tmp/replay-catalog.tar.gz -C /tmp/
    install -m644 /tmp/catalog.sqlite /usr/local/bin/catalog.sqlite
else
    echo "warning: catalog tarball missing — service will fail to start without /usr/local/bin/catalog.sqlite" >&2
fi

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
Environment=REPLAY_PORT=8080
Environment=REPLAY_HTTPS_PORT=8443
Environment=REPLAY_EXTRA_ARGS=
EnvironmentFile=-/etc/default/replay-control
ExecStart=/usr/local/bin/replay-control-app \
    --port ${REPLAY_PORT} \
    --https-port ${REPLAY_HTTPS_PORT} \
    --site-root ${REPLAY_SITE_ROOT} \
    $REPLAY_EXTRA_ARGS
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

# HTTPS port for the main app
REPLAY_HTTPS_PORT=8443

# Path to static site assets
REPLAY_SITE_ROOT=/usr/local/share/replay/site

# Extra CLI args. Dangerous debug flags must be explicit here, for example:
#REPLAY_EXTRA_ARGS=--dangerous-disable-https --dangerous-allow-insecure-auth-over-http
REPLAY_EXTRA_ARGS=

# Uncomment to override auto-detected storage path
#REPLAY_STORAGE_PATH=/media/sd

# Uncomment to override auto-detected config path
#REPLAY_CONFIG_PATH=/media/sd/config/replay.cfg

# Advanced: hash-identification workers for library rebuilds/rescans.
# Default is 2 workers for every storage class. Valid range: 1-4.
#REPLAY_CONTROL_IDENTITY_WORKERS=2

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
    <type>_https._tcp</type>
    <port>8443</port>
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
rm -f /tmp/replay-control-app-aarch64-linux.tar.gz /tmp/replay-site.tar.gz /tmp/replay-catalog.tar.gz /tmp/replay-control-app /tmp/catalog.sqlite
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
    echo "  Open ${GREEN}https://${PI_ADDR}:${DEFAULT_HTTPS_PORT}${RESET} in your browser."
    echo ""
}

# ── Uninstall / purge ───────────────────────────────────────────────────────

# Confirms the destructive --purge action up front. Aborts the script if the
# user declines. Idempotent for plain --uninstall (PURGE_DATA=false).
maybe_confirm_purge() {
    $PURGE_DATA || return 0
    $DRY_RUN && return 0
    echo ""
    warn "${BOLD}--purge${RESET}${YELLOW} will delete all Replay Control data on the Pi:${RESET}"
    echo "  - ${ENV_FILE}"
    echo "  - /var/lib/replay-control/  (central per-storage library DBs)"
    for root in "${REPLAY_STORAGE_ROOTS[@]}"; do
        echo "  - ${root}/.replay-control/  (user_data.db, downloaded media, storage-id)"
    done
    echo "ROMs, saves, captures, and BIOS files are NOT touched."
    echo ""
    if ! confirm_destructive "Proceed with --purge?"; then
        fatal "Aborted by user"
    fi
}

uninstall_local() {
    maybe_confirm_purge

    if $DRY_RUN; then
        dry "Would stop and disable ${SERVICE_NAME}"
        dry "Would remove: ${SERVICE_FILE} ${AVAHI_FILE} ${INSTALL_DIR}/replay-control-app ${INSTALL_DIR}/catalog.sqlite"
        dry "Would remove: ${SITE_DIR}"
        dry "Would run: systemctl daemon-reload"
        if $PURGE_DATA; then
            dry "Would remove: ${ENV_FILE}"
            dry "Would remove: /var/lib/replay-control/ (per-storage library DBs + host-global external_metadata.db + cache)"
            for root in "${REPLAY_STORAGE_ROOTS[@]}"; do
                dry "Would remove: ${root}/.replay-control/ (if present)"
            done
        else
            dry "Note: ${ENV_FILE} would be preserved"
        fi
        return
    fi

    info "Uninstalling locally..."
    systemctl stop "$SERVICE_NAME" 2>/dev/null || true
    systemctl disable "$SERVICE_NAME" 2>/dev/null || true
    rm -f "$SERVICE_FILE" "$AVAHI_FILE" "$INSTALL_DIR/replay-control-app" "$INSTALL_DIR/catalog.sqlite"
    rm -rf "$SITE_DIR"
    systemctl daemon-reload

    if $PURGE_DATA; then
        info "Purging Replay Control data..."
        rm -f "$ENV_FILE"
        if [[ -d /var/lib/replay-control ]]; then
            info "  removing /var/lib/replay-control"
            rm -rf /var/lib/replay-control
        fi
        for root in "${REPLAY_STORAGE_ROOTS[@]}"; do
            local data_dir="${root}/.replay-control"
            if [[ -d "$data_dir" ]]; then
                info "  removing ${data_dir}"
                rm -rf "$data_dir"
            fi
        done
        success "Replay Control purged (binary, service files, env file, data)"
    else
        success "Replay Control uninstalled"
    fi
}

uninstall_ssh() {
    discover_pi
    check_ssh_connectivity
    maybe_confirm_purge

    if $DRY_RUN; then
        dry "Would set up SSH_ASKPASS for password automation"
        dry "Would run uninstall commands on Pi via SSH:"
        dry "  - Run: systemctl stop ${SERVICE_NAME}"
        dry "  - Run: systemctl disable ${SERVICE_NAME}"
        dry "  - Remove: ${SERVICE_FILE}"
        dry "  - Remove: ${AVAHI_FILE}"
        dry "  - Remove: ${INSTALL_DIR}/replay-control-app"
        dry "  - Remove: ${INSTALL_DIR}/catalog.sqlite"
        dry "  - Remove: ${SITE_DIR}/"
        dry "  - Run: systemctl daemon-reload"
        if $PURGE_DATA; then
            dry "  - Remove: ${ENV_FILE}"
            dry "  - Remove: /var/lib/replay-control/ (per-storage library DBs + host-global external_metadata.db + cache)"
            for root in "${REPLAY_STORAGE_ROOTS[@]}"; do
                dry "  - Remove: ${root}/.replay-control/ (if present)"
            done
        else
            dry "  Note: ${ENV_FILE} would be preserved"
        fi
        return
    fi

    setup_askpass

    if $PURGE_DATA; then
        info "Purging Replay Control from Pi (binary, service files, env file, data)..."
    else
        info "Uninstalling from Pi..."
    fi

    # Pass PURGE_DATA into the remote script via env. Storage roots are
    # hard-coded on the remote side because the array can't cross the
    # heredoc boundary cleanly.
    run_ssh PURGE_DATA="$PURGE_DATA" bash -s <<'REMOTE_UNINSTALL'
set -euo pipefail

systemctl stop replay-control 2>/dev/null || true
systemctl disable replay-control 2>/dev/null || true
rm -f /etc/systemd/system/replay-control.service
rm -f /etc/avahi/services/replay-control.service
rm -f /usr/local/bin/replay-control-app
rm -f /usr/local/bin/catalog.sqlite
rm -rf /usr/local/share/replay
systemctl daemon-reload

if [[ "${PURGE_DATA:-false}" == "true" ]]; then
    rm -f /etc/default/replay-control
    if [[ -d /var/lib/replay-control ]]; then
        echo "Removing /var/lib/replay-control/"
        rm -rf /var/lib/replay-control
    fi
    for root in /media/usb /media/nvme /media/sd /media/nfs; do
        if [[ -d "${root}/.replay-control" ]]; then
            echo "Removing ${root}/.replay-control/"
            rm -rf "${root}/.replay-control"
        fi
    done
    echo "Replay Control fully purged."
else
    echo "Note: /etc/default/replay-control was preserved (remove manually if desired)"
fi
REMOTE_UNINSTALL

    teardown_askpass

    if $PURGE_DATA; then
        success "Replay Control purged from Pi"
    else
        success "Replay Control uninstalled from Pi"
    fi
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

    # Auto-detect: if running on a RePlayOS Pi and no explicit mode was chosen, install locally.
    if [[ "$MODE" == "ssh" ]] && [[ -z "$PI_ADDR" ]] && is_running_on_pi; then
        MODE="local"
        info "Detected RePlayOS — installing locally (no SSH needed)"
    fi

    case "${ACTION}-${MODE}" in
        install-local)
            install_local
            ;;
        install-ssh)
            install_ssh
            ;;
        uninstall-local)
            uninstall_local
            ;;
        uninstall-ssh)
            uninstall_ssh
            ;;
    esac
}

main "$@"
