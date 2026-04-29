#!/usr/bin/env python3
"""
Mock GitHub API server for auto-update testing.

Serves fake GitHub release API responses and downloadable tar.gz assets.
The app discovers this server via REPLAY_GITHUB_API_URL env var.

Usage:
    python3 tests/container/mock_github.py --port 9999
"""

import argparse
import hashlib
import io
import json
import os
import re
import tarfile
from http.server import HTTPServer, BaseHTTPRequestHandler
from pathlib import Path

REPO = "lapastillaroja/replay-control"
MOCK_COMMIT = "abc1234"


def _read_cargo_version() -> str:
    """Read the app version from Cargo.toml."""
    cargo_toml = Path(__file__).resolve().parents[2] / "replay-control-app" / "Cargo.toml"
    match = re.search(r'^version\s*=\s*"([^"]+)"', cargo_toml.read_text(), re.MULTILINE)
    return match.group(1) if match else "0.1.0"


def _derive_mock_versions(version: str) -> tuple[str, str]:
    """Derive stable and beta mock versions that are always newer than the app.

    Stable = minor + 1 (e.g., 0.1.0 → 0.2.0)
    Beta   = minor + 2 prerelease (e.g., 0.1.0 → 0.3.0-beta.1)
    """
    parts = version.split(".")
    major, minor = int(parts[0]), int(parts[1])
    return f"{major}.{minor + 1}.0", f"{major}.{minor + 2}.0-beta.1"


MOCK_STABLE_VERSION, MOCK_BETA_VERSION = _derive_mock_versions(_read_cargo_version())
# Keep MOCK_VERSION as alias for beta (backward compat with tests)
MOCK_VERSION = MOCK_BETA_VERSION


def make_dummy_binary() -> bytes:
    """Create a shell script that mimics the app's /api/version endpoint."""
    script = f"""#!/bin/sh
# Dummy replay-control-app binary for testing.
# Serves a simple HTTP response on the requested port.
echo '{{"version":"{MOCK_STABLE_VERSION}","commit":"{MOCK_COMMIT}"}}'
"""
    return script.encode()


def make_tarball(name: str, content: bytes) -> bytes:
    """Create a tar.gz containing a single file."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        info = tarfile.TarInfo(name=name)
        info.size = len(content)
        info.mode = 0o755
        tar.addfile(info, io.BytesIO(content))
    return buf.getvalue()


def make_site_tarball() -> bytes:
    """Create a tar.gz containing a minimal site directory.

    Mirrors build.sh's hashing convention: WASM + JS get content-hashed
    filenames and a hash.txt sidecar so the post-update server can resolve
    Leptos's hashed asset URLs.
    """
    wasm_bytes = b"\x00"
    wasm_hash = hashlib.sha256(wasm_bytes).hexdigest()[:16]
    wasm_name = f"replay_control_app.{wasm_hash}.wasm"

    # The wasm-bindgen JS imports the wasm by name; build.sh sed-replaces
    # `replay_control_app_bg.wasm` with the hashed name. Mimic that here.
    js_bytes = f"// placeholder importing ./{wasm_name}\n".encode()
    js_hash = hashlib.sha256(js_bytes).hexdigest()[:16]
    js_name = f"replay_control_app.{js_hash}.js"

    hash_txt = f"js: {js_hash}\nwasm: {wasm_hash}\n".encode()

    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        for path, content in [
            (f"site/pkg/{js_name}", js_bytes),
            (f"site/pkg/{wasm_name}", wasm_bytes),
            ("site/hash.txt", hash_txt),
            ("site/style.css", b"/* placeholder */\n"),
        ]:
            info = tarfile.TarInfo(name=path)
            info.size = len(content)
            info.mode = 0o644
            tar.addfile(info, io.BytesIO(content))
    return buf.getvalue()


def make_catalog_tarball() -> bytes:
    """Create a tar.gz containing a stub catalog.sqlite.

    The real file is a SQLite DB, but for the swap+restart path we only need
    the helper script to find a file named `catalog.sqlite` next to the binary.
    The dummy binary doesn't actually open it, so any non-empty bytes work.
    """
    return make_tarball("catalog.sqlite", b"-- stub catalog.sqlite\n")


# Pre-generate assets at module level
BINARY_TARBALL = make_tarball("replay-control-app", make_dummy_binary())
SITE_TARBALL = make_site_tarball()
CATALOG_TARBALL = make_catalog_tarball()


def release_json(tag: str, port: int, prerelease: bool = False) -> dict:
    """Build a GitHub-style release JSON object."""
    base = f"http://localhost:{port}"
    return {
        "tag_name": tag,
        "name": f"v{tag}",
        "prerelease": prerelease,
        "draft": False,
        "assets": [
            {
                "name": "replay-control-app-x86_64.tar.gz",
                "browser_download_url": f"{base}/download/replay-control-app-x86_64.tar.gz",
                "size": len(BINARY_TARBALL),
                "content_type": "application/gzip",
            },
            {
                "name": "replay-control-site.tar.gz",
                "browser_download_url": f"{base}/download/replay-control-site.tar.gz",
                "size": len(SITE_TARBALL),
                "content_type": "application/gzip",
            },
            {
                "name": "replay-catalog.tar.gz",
                "browser_download_url": f"{base}/download/replay-catalog.tar.gz",
                "size": len(CATALOG_TARBALL),
                "content_type": "application/gzip",
            },
        ],
    }


class MockGitHubHandler(BaseHTTPRequestHandler):
    """Handle GitHub API and download requests."""

    def do_GET(self):
        port = self.server.server_address[1]

        # Health check
        if self.path == "/health":
            self._json_response({"status": "ok"})
            return

        # Toggle download failures (for testing network errors)
        if self.path == "/mock/downloads/fail":
            self.server.fail_downloads = True
            self._json_response({"status": "downloads_will_fail"})
            return
        if self.path == "/mock/downloads/ok":
            self.server.fail_downloads = False
            self._json_response({"status": "downloads_restored"})
            return

        # Latest release (GitHub only returns non-prerelease here)
        if self.path == f"/repos/{REPO}/releases/latest":
            self._json_response(release_json(MOCK_STABLE_VERSION, port, prerelease=False))
            return

        # All releases (strip query params for matching)
        path_no_query = self.path.split("?")[0]
        if path_no_query == f"/repos/{REPO}/releases":
            self._json_response([
                release_json(MOCK_BETA_VERSION, port, prerelease=True),
                release_json(MOCK_STABLE_VERSION, port, prerelease=False),
            ])
            return

        # Release by tag
        prefix = f"/repos/{REPO}/releases/tags/"
        if self.path.startswith(prefix):
            tag = self.path[len(prefix):]
            self._json_response(release_json(tag, port))
            return

        # Binary download
        if self.path == "/download/replay-control-app-x86_64.tar.gz":
            if getattr(self.server, "fail_downloads", False):
                self.send_error(503, "Service Unavailable")
                return
            self._binary_response(BINARY_TARBALL, "application/gzip")
            return

        # Site download
        if self.path == "/download/replay-control-site.tar.gz":
            if getattr(self.server, "fail_downloads", False):
                self.send_error(503, "Service Unavailable")
                return
            self._binary_response(SITE_TARBALL, "application/gzip")
            return

        # Catalog download
        if self.path == "/download/replay-catalog.tar.gz":
            if getattr(self.server, "fail_downloads", False):
                self.send_error(503, "Service Unavailable")
                return
            self._binary_response(CATALOG_TARBALL, "application/gzip")
            return

        self.send_error(404, f"Not found: {self.path}")

    def _json_response(self, data):
        body = json.dumps(data).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _binary_response(self, data: bytes, content_type: str):
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, format, *args):
        """Suppress default request logging for cleaner output."""
        pass


def main():
    parser = argparse.ArgumentParser(description="Mock GitHub API server")
    parser.add_argument("--port", type=int, default=9999)
    args = parser.parse_args()

    server = HTTPServer(("0.0.0.0", args.port), MockGitHubHandler)
    print(f"Mock GitHub server listening on port {args.port}")
    print(f"  Stable release: v{MOCK_STABLE_VERSION}")
    print(f"  Beta release:   v{MOCK_BETA_VERSION}")
    print(f"  Binary asset:   {len(BINARY_TARBALL)} bytes")
    print(f"  Site asset:     {len(SITE_TARBALL)} bytes")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    server.server_close()


if __name__ == "__main__":
    main()
