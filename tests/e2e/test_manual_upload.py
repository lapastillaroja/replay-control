"""
End-to-end coverage for manual (PDF/TXT) uploads.

The upload endpoint is a plain axum multipart route, `POST /manuals/upload/<system>`,
with fields `file`, `rom_filename`, `base_title` (+ optional `title`, `language`).
Accepted bytes are a real PDF (`%PDF-`) or valid UTF-8 text; the extension must be
`.pdf` or `.txt`. Files land in `.replay-control/manuals/<system>/`.

These tests POST the endpoint directly (the container runs auth-free standalone
mode) and assert both the HTTP contract and the on-disk effect. Container only —
they mutate `/media/usb`.
"""

from urllib.request import Request, urlopen

import pytest

from conftest import CONTAINER, MANUALS_DIR, PI_URL, list_files

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="manual-upload e2e mutates container storage and is unsafe for Pi targets",
)

MIN_PDF = b"%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF\n"


def _multipart(fields: dict, file_name: str, file_bytes: bytes) -> tuple[bytes, str]:
    boundary = "----e2eBoundary7MA4YWxkTrZu0gW"
    out = bytearray()
    for name, value in fields.items():
        out += f"--{boundary}\r\n".encode()
        out += f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode()
        out += f"{value}\r\n".encode()
    out += f"--{boundary}\r\n".encode()
    out += (
        f'Content-Disposition: form-data; name="file"; filename="{file_name}"\r\n'
        "Content-Type: application/octet-stream\r\n\r\n"
    ).encode()
    out += file_bytes + b"\r\n"
    out += f"--{boundary}--\r\n".encode()
    return bytes(out), f"multipart/form-data; boundary={boundary}"


def _upload(system: str, file_name: str, file_bytes: bytes, *, rom_filename: str,
            base_title: str = "E2E Seed Game") -> int:
    body, content_type = _multipart(
        {"rom_filename": rom_filename, "base_title": base_title},
        file_name,
        file_bytes,
    )
    req = Request(
        # The upload router is nested under /api (see api/mod.rs).
        f"{PI_URL}/api/manuals/upload/{system}",
        data=body,
        method="POST",
        headers={"Content-Type": content_type},
    )
    try:
        return urlopen(req, timeout=20).status
    except Exception as exc:  # noqa: BLE001
        return getattr(exc, "code", 0)


def test_upload_valid_pdf_persists(seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    before = list_files(f"{MANUALS_DIR}/{system}")
    status = _upload(system, "manual.pdf", MIN_PDF, rom_filename=rom)
    assert status == 200, f"valid PDF upload should return 200, got {status}"
    after = list_files(f"{MANUALS_DIR}/{system}")
    assert len(after) == len(before) + 1, f"expected one new manual file: {before} -> {after}"
    assert after[0].endswith(".pdf")


def test_upload_valid_txt_persists(seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    status = _upload(system, "notes.txt", b"plain text manual\n", rom_filename=rom)
    assert status == 200, f"valid TXT upload should return 200, got {status}"
    after = list_files(f"{MANUALS_DIR}/{system}")
    assert any(name.endswith(".txt") for name in after), f"txt manual not stored: {after}"


def test_upload_rejects_binary_bytes_named_pdf(seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    # Not a PDF and not valid UTF-8 -> validate_manual_bytes returns None -> 400.
    status = _upload(system, "fake.pdf", b"\xff\xfe\x00\x01not a pdf", rom_filename=rom)
    assert status == 400, f"non-PDF bytes should be rejected with 400, got {status}"
    assert not list_files(f"{MANUALS_DIR}/{system}"), "rejected upload must not persist a file"


def test_upload_rejects_disallowed_extension(seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    status = _upload(system, "evil.exe", MIN_PDF, rom_filename=rom)
    assert status == 400, f"disallowed extension should be rejected with 400, got {status}"


def test_upload_rejects_path_traversal_rom_filename(seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    status = _upload(system, "manual.pdf", MIN_PDF, rom_filename="../escape.nes")
    assert status == 400, f"path-traversal rom_filename should be rejected with 400, got {status}"


def test_upload_rejects_missing_base_title(seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    status = _upload(system, "manual.pdf", MIN_PDF, rom_filename=rom, base_title="")
    assert status == 400, f"empty base_title should be rejected with 400, got {status}"
