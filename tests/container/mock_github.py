#!/usr/bin/env python3
"""
Mock GitHub API server for auto-update testing.

Serves fake GitHub release API responses and downloadable tar.gz assets.
The app discovers this server via REPLAY_GITHUB_API_URL env var.

Usage:
    python3 tests/container/mock_github.py --port 9999
"""

import argparse
import io
import json
import os
import tarfile
from http.server import HTTPServer, BaseHTTPRequestHandler

REPO = "lapastillaroja/replay-control"
MOCK_VERSION = "0.1.0-beta.4"
MOCK_COMMIT = "abc1234"


def make_dummy_binary() -> bytes:
    """Create a shell script that mimics the app's /api/version endpoint."""
    script = f"""#!/bin/sh
# Dummy replay-control-app binary for testing.
# Serves a simple HTTP response on the requested port.
echo '{{"version":"{MOCK_VERSION}","commit":"{MOCK_COMMIT}"}}'
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
    """Create a tar.gz containing a minimal site directory."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        # Create site/pkg/ directory with a dummy file
        for path, content in [
            ("site/pkg/replay_control_app.js", b"// placeholder\n"),
            ("site/pkg/replay_control_app_bg.wasm", b"\x00"),
            ("site/style.css", b"/* placeholder */\n"),
        ]:
            info = tarfile.TarInfo(name=path)
            info.size = len(content)
            info.mode = 0o644
            tar.addfile(info, io.BytesIO(content))
    return buf.getvalue()


# Pre-generate assets at module level
BINARY_TARBALL = make_tarball("replay-control-app", make_dummy_binary())
SITE_TARBALL = make_site_tarball()


def release_json(tag: str, port: int) -> dict:
    """Build a GitHub-style release JSON object."""
    base = f"http://localhost:{port}"
    return {
        "tag_name": tag,
        "name": f"v{tag}",
        "prerelease": True,
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

        # Latest release (GitHub only returns non-prerelease here)
        if self.path == f"/repos/{REPO}/releases/latest":
            # No stable releases exist — only prereleases
            self.send_error(404, "Not Found")
            return

        # All releases (strip query params for matching)
        path_no_query = self.path.split("?")[0]
        if path_no_query == f"/repos/{REPO}/releases":
            self._json_response([release_json(MOCK_VERSION, port)])
            return

        # Release by tag
        prefix = f"/repos/{REPO}/releases/tags/"
        if self.path.startswith(prefix):
            tag = self.path[len(prefix):]
            self._json_response(release_json(tag, port))
            return

        # Binary download
        if self.path == "/download/replay-control-app-x86_64.tar.gz":
            self._binary_response(BINARY_TARBALL, "application/gzip")
            return

        # Site download
        if self.path == "/download/replay-control-site.tar.gz":
            self._binary_response(SITE_TARBALL, "application/gzip")
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
    print(f"  Latest release: v{MOCK_VERSION}")
    print(f"  Binary asset:   {len(BINARY_TARBALL)} bytes")
    print(f"  Site asset:     {len(SITE_TARBALL)} bytes")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    server.server_close()


if __name__ == "__main__":
    main()
