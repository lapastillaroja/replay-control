"""
End-to-end coverage for the static media serving routes.

These root routes stream user files off disk: `/captures/*` (screenshots),
`/owned-manuals/*` (uploaded manuals), and `/media/*` (downloaded box art). They
set Content-Type by extension and 404 on missing files. This complements the
capture/manual *deletion* tests by exercising the *serving* side. Container only.
"""

from urllib.error import HTTPError, URLError
from urllib.request import urlopen

import pytest

from conftest import (
    CAPTURES_DIR,
    CONTAINER,
    MANUALS_DIR,
    MEDIA_DIR,
    PI_URL,
    exec_cmd,
)

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="media-serving e2e seeds files in container storage; container only",
)


def _get(path: str) -> tuple[int, str]:
    """GET a served file; return (status, Content-Type)."""
    try:
        resp = urlopen(f"{PI_URL}{path}", timeout=10)
        return resp.status, resp.headers.get("Content-Type", "")
    except HTTPError as exc:
        return exc.code, ""
    except URLError:
        return 0, ""


def test_media_routes_serve_seeded_files_and_404_missing(seeded_game):
    system = seeded_game["system"]
    # Content-Type is chosen by extension, so file bytes don't need to be valid.
    exec_cmd(
        f'mkdir -p "{CAPTURES_DIR}/{system}" "{MANUALS_DIR}/{system}" "{MEDIA_DIR}/{system}/box-art"; '
        f'printf x > "{CAPTURES_DIR}/{system}/shot.png"; '
        f'printf x > "{MANUALS_DIR}/{system}/manual.pdf"; '
        f'printf x > "{MEDIA_DIR}/{system}/box-art/cover.png"'
    )

    status, ctype = _get(f"/captures/{system}/shot.png")
    assert status == 200 and "image/png" in ctype, f"capture serve: {status} {ctype!r}"

    status, ctype = _get(f"/owned-manuals/{system}/manual.pdf")
    assert status == 200 and "application/pdf" in ctype, f"manual serve: {status} {ctype!r}"

    status, ctype = _get(f"/media/{system}/box-art/cover.png")
    assert status == 200 and "image/png" in ctype, f"media serve: {status} {ctype!r}"

    status, _ = _get(f"/captures/{system}/does-not-exist.png")
    assert status == 404, f"missing capture should 404, got {status}"
