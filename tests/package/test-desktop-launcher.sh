#!/bin/bash
set -euo pipefail

launcher=/usr/lib/replay-control/open-browser
config=/etc/default/replay-control
work=$(mktemp -d)
cp "$config" "$work/original-config"
cleanup() {
    cp "$work/original-config" "$config"
    rm -rf "$work"
}
trap cleanup EXIT

mkdir "$work/xdg" "$work/gio"
cat > "$work/xdg/xdg-open" <<'EOF'
#!/bin/sh
printf '%s\n' "$1" > "$REPLAY_LAUNCH_TEST_OUT"
EOF
cat > "$work/gio/gio" <<'EOF'
#!/bin/sh
test "$1" = open || exit 1
printf '%s\n' "$2" > "$REPLAY_LAUNCH_TEST_OUT"
EOF
chmod +x "$work/xdg/xdg-open" "$work/gio/gio"
export REPLAY_LAUNCH_TEST_OUT="$work/url"

printf 'REPLAY_PORT=8080\nREPLAY_HTTPS_PORT=8443\n' > "$config"
PATH="$work/xdg" "$launcher"
grep -Fxq 'https://localhost:8443/' "$work/url"

printf 'REPLAY_PORT=8080\nREPLAY_HTTPS_PORT="9443"\nREPLAY_STORAGE_PATH=/srv/My Games\n' > "$config"
PATH="$work/xdg" "$launcher"
grep -Fxq 'https://localhost:9443/' "$work/url"

printf 'REPLAY_PORT=9080\nREPLAY_HTTPS_PORT=9443\n' > "$config"
PATH="$work/gio" "$launcher"
grep -Fxq 'https://localhost:9443/' "$work/url"

printf "REPLAY_HTTPS_PORT='9553'\n" > "$config"
PATH="$work/xdg" "$launcher"
grep -Fxq 'https://localhost:9553/' "$work/url"
