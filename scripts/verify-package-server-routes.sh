#!/usr/bin/env bash
set -euo pipefail

# Leptos hashes server function routes using CARGO_MANIFEST_DIR. Keep the
# checkout path and Rust toolchain the same for native and browser builds;
# a 1.95 native binary did not match a 1.98 browser bundle at the same path.
binary=${1:?usage: verify-package-server-routes.sh BINARY SITE_DIR}
site=${2:?usage: verify-package-server-routes.sh BINARY SITE_DIR}
hash_file="$site/hash.txt"
[[ -s "$binary" && -s "$hash_file" ]] || {
    echo 'Release binary or site hash manifest is missing' >&2
    exit 1
}
wasm_hash=$(sed -n 's/^wasm: //p' "$hash_file")
[[ "$wasm_hash" =~ ^[0-9a-f]{16}$ ]] || {
    echo 'Invalid WASM hash in site manifest' >&2
    exit 1
}
wasm="$site/pkg/replay_control_app.$wasm_hash.wasm"
[[ -s "$wasm" ]] || {
    echo "WASM asset is missing: $wasm" >&2
    exit 1
}

python3 - "$binary" "$wasm" <<'PY'
import pathlib
import re
import struct
import sys


def routes(path: str, length_bytes: int) -> set[str]:
    data = pathlib.Path(path).read_bytes()
    found = set()
    for match in re.finditer(rb"/sfn/[a-z_][a-z_0-9]*[0-9]{5,}", data):
        route = match.group()
        # Rust stores the string length immediately before the bytes. A route
        # can abut another ASCII digit in the binary; scanning until a non-digit
        # would then invent a different server function hash.
        if match.start() >= length_bytes:
            fmt = "<I" if length_bytes == 4 else "<Q"
            declared_length = struct.unpack_from(fmt, data, match.start() - length_bytes)[0]
            if 0 < declared_length <= len(route):
                route = route[:declared_length]
        suffix = re.search(rb"[0-9]+$", route)
        if suffix is None:
            continue
        digits = suffix.group()
        while digits and int(digits) > (1 << 64) - 1:
            digits = digits[:-1]
        if digits:
            found.add((route[:suffix.start()] + digits).decode("ascii"))
    if not found:
        raise SystemExit(f"No server function routes found in {path}")
    return found


native = routes(sys.argv[1], 8)
browser = routes(sys.argv[2], 4)
missing = sorted(browser - native)
if missing:
    print("Browser WASM calls server function routes absent from the packaged binary:", file=sys.stderr)
    print("\n".join(missing[:10]), file=sys.stderr)
    print("Build the browser WASM and native binary from the same checkout path.", file=sys.stderr)
    raise SystemExit(1)
PY
