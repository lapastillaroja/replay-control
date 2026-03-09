# Binary Distribution Plan

Distribute pre-built Replay Control binaries via GitHub without publishing source code.

## Options Compared

| Option | Public downloads | No source exposed | Free | Complexity |
|--------|:---:|:---:|:---:|:---:|
| **A. Public repo for releases only** | Yes | Yes | Yes | Low |
| B. GitHub Packages (container) | Yes | Yes | Yes | Medium |
| C. GitHub Pages as file host | Yes | Yes | Yes | Medium |
| D. External hosting (R2/S3) | Yes | Yes | No* | Medium |

\* R2 has a free tier that would cover this use case, but adds operational complexity.

**Recommendation: Option A** -- a minimal public repo (e.g. `replay-releases`) containing
only a README and the install script. The private `replay` repo's CI builds binaries and
pushes them as GitHub Releases to the public repo. This is the standard pattern for
closed-source projects distributing via GitHub.

## Architecture

```
[Private repo: replay]          [Public repo: replay-releases]
  push tag v0.3.0                    README.md
       |                             install.sh
  GitHub Actions                     Releases/
    build WASM                         v0.3.0/
    build aarch64 binary                 replay-aarch64-linux.tar.gz
    package tarball                      checksums.txt
    push release ──────────────>
```

## Packaging Format

Single tarball: `replay-aarch64-linux.tar.gz` containing:

```
replay-app          # aarch64 server binary
site/               # WASM + CSS + static assets
  pkg/
    replay_app.js
    replay_app_bg.wasm
  style.css
  icons/
```

The install script already handles extracting two separate archives. Simplify to one:
the CI packages everything into a single tarball, and `install.sh` extracts it.

Estimated size: ~5-10 MB (Rust binary ~4 MB + WASM + assets).

## CI/CD Pipeline

### Private repo workflow (`.github/workflows/release.yml`)

Triggered by version tags. Builds both WASM and the aarch64 server binary, packages
them, and creates a release on the public repo using a PAT.

```yaml
name: Release
on:
  push:
    tags: ["v*"]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown

      - name: Install wasm-bindgen-cli
        run: cargo install wasm-bindgen-cli

      - name: Build WASM (hydrate)
        run: |
          cargo build -p replay-app --lib \
            --target wasm32-unknown-unknown \
            --release --features hydrate --no-default-features

      - name: Run wasm-bindgen
        run: |
          mkdir -p target/site/pkg
          wasm-bindgen target/wasm32-unknown-unknown/release/replay_app.wasm \
            --out-dir target/site/pkg --out-name replay_app --target web --no-typescript

      - name: Copy static assets
        run: |
          cp replay-app/style/style.css target/site/style.css
          cp -r replay-app/static/icons target/site/icons 2>/dev/null || true

      - name: Build aarch64 binary
        uses: houseabsolute/actions-rust-cross@v0
        with:
          command: build
          target: aarch64-unknown-linux-gnu
          args: "-p replay-app --bin replay-app --release --features ssr --no-default-features"

      - name: Package tarball
        run: |
          STAGING="replay-aarch64-linux"
          mkdir -p "$STAGING"
          cp target/aarch64-unknown-linux-gnu/release/replay-app "$STAGING/"
          cp -r target/site "$STAGING/site"
          tar -czf replay-aarch64-linux.tar.gz "$STAGING"
          sha256sum replay-aarch64-linux.tar.gz > checksums.txt

      - name: Create release on public repo
        uses: softprops/action-gh-release@v2
        with:
          repository: user/replay-releases
          token: ${{ secrets.RELEASES_PAT }}
          tag_name: ${{ github.ref_name }}
          name: ${{ github.ref_name }}
          files: |
            replay-aarch64-linux.tar.gz
            checksums.txt
          body: "Replay Control ${{ github.ref_name }}"
```

### Key CI details

- **Cross-compilation**: `houseabsolute/actions-rust-cross` handles the aarch64 toolchain
  and linker setup automatically (uses `cross` under the hood with Docker).
- **WASM build**: runs natively on the x86 runner (wasm32 is a platform-independent target).
- **PAT setup**: create a fine-grained PAT with `Contents: write` permission scoped to
  the `replay-releases` repo. Store it as `RELEASES_PAT` in the private repo's secrets.
- **Manual trigger**: add `workflow_dispatch:` to the `on:` block for manual releases.

## Install Experience

### One-liner

```bash
curl -fsSL https://raw.githubusercontent.com/user/replay-releases/main/install.sh | bash
```

### Install script changes

Update `install.sh` to point `REPO` at the public releases repo:

```bash
REPO="user/replay-releases"
```

Simplify `resolve_download_urls` and `download_artifacts` to use a single tarball:

```bash
resolve_download_urls() {
    local base_url
    if [[ "$VERSION" == "latest" ]]; then
        base_url="https://github.com/$REPO/releases/latest/download"
    else
        base_url="https://github.com/$REPO/releases/download/$VERSION"
    fi
    TARBALL_URL="${base_url}/replay-aarch64-linux.tar.gz"
    CHECKSUM_URL="${base_url}/checksums.txt"
}
```

After downloading, verify the checksum:

```bash
(cd "$TMPDIR_WORK" && sha256sum -c checksums.txt)
```

## Version Management

- Tag the private repo: `git tag v0.3.0 && git push --tags`
- CI creates a matching release on the public repo.
- `install.sh` defaults to `latest` (GitHub's `/releases/latest/download/` redirect).
- Pin a version: `REPLAY_VERSION=v0.3.0 bash install.sh`
- The binary should embed its version (set via `env!("CARGO_PKG_VERSION")` or build-time
  env var) so users can check: `replay-app --version`.

## Security

- **Checksums**: `sha256sum` file uploaded alongside each release. The install script
  verifies after download.
- **Signing** (optional, later): sign the tarball with a GPG key or use cosign. Add the
  public key to the install script or the public repo README.
- **HTTPS only**: GitHub releases are served over HTTPS. The install script uses
  `curl -fsSL` which fails on non-HTTPS or certificate errors.

## Setup Checklist

1. Create public repo `user/replay-releases` with README and install.sh
2. Create a fine-grained PAT (Contents: write) scoped to that repo
3. Add `RELEASES_PAT` secret to the private `replay` repo
4. Add `.github/workflows/release.yml` to the private repo (see above)
5. Update `REPO` in `install.sh` to point to the public repo
6. Consolidate the two tarballs into one in the build/install flow
7. Add `--version` flag to `replay-app` if not already present
8. Tag and push: `git tag v0.1.0 && git push --tags`
9. Verify: `curl -fsSL https://raw.githubusercontent.com/user/replay-releases/main/install.sh | bash`
