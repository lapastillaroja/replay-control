#!/bin/bash
# Measure CPU usage of replay-control on the Pi over SSH.
# Uses the same askpass + StrictHostKeyChecking=no pattern as dev.sh / pi-memory.sh.
#
# Reads /proc/<pid>/stat (utime + stime) at two timestamps and computes
# Δticks / (Δwall_ticks * NCPU) for the service. The CPU% column reports
# percent of one core (so 100% = one core saturated, 400% = the Pi 5 fully busy).
# Total host CPU% (all processes) is also reported for context.
#
# Usage:
#   tools/pi-cpu.sh                              # 10s sample, idle by default
#   tools/pi-cpu.sh --duration 30                # sample for 30s
#   tools/pi-cpu.sh --browse                     # generate "one user browsing"
#                                                # traffic in the background
#   tools/pi-cpu.sh --browse --duration 30
#   tools/pi-cpu.sh --ip 192.168.1.50            # override IP/hostname
#   tools/pi-cpu.sh --json                       # emit a compact JSON line
#
# Environment:
#   PI_USER     ssh user (default: root)
#   PI_PASS     ssh password (default: replayos)
#   PI_IP       Pi address, same as --ip (default: replay.local)
#   PI_PORT     HTTP port (default: 8080)
#
# The --browse load mimics one casual user: ~1 request every 2 seconds across
# home, a system page, a game detail, the manuals deep link, and search. It
# is intentionally light — under heavy load use load-test.sh instead.

set -euo pipefail

PI_USER="${PI_USER:-root}"
PI_PASSWORD="${PI_PASS:-replayos}"
PI_IP="${PI_IP:-replay.local}"
PI_PORT="${PI_PORT:-8080}"
SERVICE="replay-control"
SSH_OPTS="-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10"

DURATION=10
BROWSE=false
JSON=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --duration) DURATION="$2"; shift 2 ;;
        --browse)   BROWSE=true; shift ;;
        --ip)       PI_IP="$2"; shift 2 ;;
        --port)     PI_PORT="$2"; shift 2 ;;
        --json)     JSON=true; shift ;;
        -h|--help)  sed -n '2,/^$/p' "$0" | sed 's/^# \?//'; exit 0 ;;
        *)          echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done

ASKPASS="$(mktemp)"
trap 'rm -f "$ASKPASS"; [[ -n "${BROWSE_PID:-}" ]] && kill "$BROWSE_PID" 2>/dev/null || true' EXIT
printf '#!/bin/sh\necho "%s"\n' "$PI_PASSWORD" > "$ASKPASS"
chmod +x "$ASKPASS"

run_ssh() {
    SSH_ASKPASS="$ASKPASS" SSH_ASKPASS_REQUIRE=force DISPLAY= \
        ssh $SSH_OPTS "${PI_USER}@${PI_IP}" "$@"
}

# HTTPS / auth: set PI_SCHEME=https (+ PI_PORT=8443) for https-by-default builds,
# and PI_COOKIE="ReplayControlSession=..." so the browse load hits the real app
# instead of a login redirect / "Use HTTPS" page. Wrapping curl applies both.
PI_SCHEME="${PI_SCHEME:-http}"
CURL_EXTRA=()
[[ "$PI_SCHEME" == https ]] && CURL_EXTRA+=(--insecure)
[[ -n "${PI_COOKIE:-}" ]] && CURL_EXTRA+=(--cookie "$PI_COOKIE")
curl() { command curl "${CURL_EXTRA[@]}" "$@"; }

# Sanity check.
if ! curl -s -o /dev/null --max-time 5 "${PI_SCHEME}://${PI_IP}:${PI_PORT}/manifest.json" 2>/dev/null; then
    echo "ERROR: replay-control not reachable at ${PI_SCHEME}://${PI_IP}:${PI_PORT}" >&2
    exit 1
fi

# Background browsing load: one user clicking through pages every ~2s.
if $BROWSE; then
    BROWSE_PATHS=(
        "/"
        "/games/nintendo_nes"
        "/games/sega_genesis"
        "/search?q=mario"
        "/games/snes/Super%20Mario%20World%20%28USA%29.sfc"
        "/games/snes/Super%20Mario%20World%20%28USA%29.sfc#manuals"
    )
    BASE="${PI_SCHEME}://${PI_IP}:${PI_PORT}"
    (
        while true; do
            for p in "${BROWSE_PATHS[@]}"; do
                curl -s -o /dev/null --compressed --max-time 30 "${BASE}${p}" || true
                sleep 2
            done
        done
    ) &
    BROWSE_PID=$!
    # Give the loop a moment to start hitting the server.
    sleep 1
fi

# Sample CPU on the Pi: read jiffies before, sleep, read after.
output=$(run_ssh "\
    set -e; \
    PID=\$(systemctl show $SERVICE -p MainPID --value); \
    if [ -z \"\$PID\" ] || [ \"\$PID\" = 0 ]; then \
        echo 'ERROR: no PID for $SERVICE' >&2; exit 1; \
    fi; \
    NCPU=\$(nproc); \
    HZ=\$(getconf CLK_TCK); \
    read u1 s1 < <(awk '{print \$14, \$15}' /proc/\$PID/stat); \
    read t1 < <(awk '/^cpu /{idle=\$5+\$6; total=0; for(i=2;i<=NF;i++) total+=\$i; print total\" \"idle}' /proc/stat); \
    sleep $DURATION; \
    read u2 s2 < <(awk '{print \$14, \$15}' /proc/\$PID/stat); \
    read t2 < <(awk '/^cpu /{idle=\$5+\$6; total=0; for(i=2;i<=NF;i++) total+=\$i; print total\" \"idle}' /proc/stat); \
    proc_d=\$(( (u2 + s2) - (u1 + s1) )); \
    set -- \$t1; total1=\$1; idle1=\$2; \
    set -- \$t2; total2=\$1; idle2=\$2; \
    total_d=\$(( total2 - total1 )); \
    idle_d=\$(( idle2 - idle1 )); \
    proc_pct_one_core=\$(awk -v p=\$proc_d -v hz=\$HZ -v d=$DURATION 'BEGIN{ printf \"%.2f\", (p/hz/d)*100 }'); \
    proc_pct_all_cores=\$(awk -v p=\$proc_d -v hz=\$HZ -v d=$DURATION -v n=\$NCPU 'BEGIN{ printf \"%.2f\", (p/hz/d/n)*100 }'); \
    host_pct=\$(awk -v t=\$total_d -v i=\$idle_d 'BEGIN{ if(t>0) printf \"%.2f\", (1-i/t)*100; else print \"0.00\" }'); \
    LOAD=\$(awk '{print \$1\" \"\$2\" \"\$3}' /proc/loadavg); \
    echo \"PID=\$PID\"; \
    echo \"NCPU=\$NCPU\"; \
    echo \"DURATION=$DURATION\"; \
    echo \"PROC_PCT_ONE_CORE=\$proc_pct_one_core\"; \
    echo \"PROC_PCT_ALL_CORES=\$proc_pct_all_cores\"; \
    echo \"HOST_PCT=\$host_pct\"; \
    echo \"LOADAVG=\$LOAD\"; \
")

if $JSON; then
    pid=$(awk -F= '/^PID=/{print $2}' <<<"$output")
    ncpu=$(awk -F= '/^NCPU=/{print $2}' <<<"$output")
    duration=$(awk -F= '/^DURATION=/{print $2}' <<<"$output")
    proc_one=$(awk -F= '/^PROC_PCT_ONE_CORE=/{print $2}' <<<"$output")
    proc_all=$(awk -F= '/^PROC_PCT_ALL_CORES=/{print $2}' <<<"$output")
    host=$(awk -F= '/^HOST_PCT=/{print $2}' <<<"$output")
    load=$(awk -F= '/^LOADAVG=/{print $2}' <<<"$output")
    mode=$([[ "$BROWSE" == "true" ]] && echo "browse" || echo "idle")
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    printf '{"ts":"%s","mode":"%s","pid":%s,"ncpu":%s,"duration_s":%s,"proc_pct_one_core":%s,"proc_pct_all_cores":%s,"host_pct":%s,"loadavg":"%s"}\n' \
        "$ts" "$mode" "$pid" "$ncpu" "$duration" "$proc_one" "$proc_all" "$host" "$load"
else
    mode=$([[ "$BROWSE" == "true" ]] && echo "low-load (single-user browse)" || echo "idle")
    echo "Mode: $mode"
    echo "$output"
fi
