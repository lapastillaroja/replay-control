#!/usr/bin/env python3
"""Read a server function route from a Leptos WASM string literal."""

import pathlib
import re
import struct
import sys


wasm = pathlib.Path(sys.argv[1]).read_bytes()
prefix = sys.argv[2].encode("ascii")
for match in re.finditer(re.escape(prefix) + rb"[0-9]{5,}", wasm):
    if match.start() < 4:
        continue
    declared_length = struct.unpack_from("<I", wasm, match.start() - 4)[0]
    if len(prefix) + 5 <= declared_length <= len(match.group()):
        print(match.group()[:declared_length].decode("ascii"))
        break
else:
    raise SystemExit(f"No valid route for {sys.argv[2]}")
