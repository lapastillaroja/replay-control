# Getting Started

A quick guide to get Replay Control running on your Pi.

## Prerequisites

- A Raspberry Pi running [RePlayOS](https://www.replayos.com/)
- ROM files for the systems you want to play
- A computer, phone, or tablet on the same network as the Pi

## Install

From any computer on the same network:

```bash
curl -sSL https://github.com/lapastillaroja/replay-control/releases/latest/download/install.sh | bash -s -- --ip replay.local
```

The installer downloads the latest release and sets everything up. If `replay.local` doesn't work, replace it with your Pi's IP address (e.g., `192.168.1.50` — check your router's admin page to find it). For more install options (SD card, specific version, custom password), see the [Installation guide](install.md).

## First Launch

Open a browser and go to:

```
http://replay.local:8080
```

If `replay.local` does not resolve, use your Pi's IP address instead (e.g., `http://192.168.1.50:8080`).

> **Tip:** To find your Pi's IP address, check your router's connected devices list, or run `hostname -I` on the Pi.

On first launch, you will see a "Scanning game library..." banner while the app indexes your ROMs. This runs in the background -- the UI is usable immediately, and your library fills in as systems are scanned.

## Add ROMs

Place ROM files in the `roms/` directory on your storage device (SD card, USB drive, or NFS share), organized by system folder. RePlayOS expects a folder per system:

```
roms/
  snes/
    Super Mario World (USA).sfc
    ...
  sega_smd/
    Sonic The Hedgehog (World).md
    ...
  arcade_fbneo/
    sf2.zip
    ...
```

On local storage (SD, USB, NVMe), new ROMs are detected automatically. On NFS, use the "Rebuild Game Library" button in the metadata page to pick up changes.

## Browse Your Library

The home page shows your recently played games, library stats, and personalized recommendations. Tap any system card to see its games, or use the search tab to find games across all systems.

You can install the app as a PWA (home screen app) on your phone for quick access.

## Enrich Your Library

The app includes built-in data for game names, genres, and player counts. For richer metadata (descriptions, ratings, box art), go to **More > Game Data**:

- **Download Metadata** -- imports game descriptions, ratings, and genres from LaunchBox
- **Download Images** -- fetches box art and screenshots from libretro-thumbnails

Both are optional and require an internet connection on the Pi.

## Next Steps

- [Game Library](game-library.md) -- browsing, favorites, multi-disc handling
- [Search](search.md) -- cross-system search and developer pages
- [Settings](settings.md) -- Wi-Fi, themes, hostname, and more
- [Feature overview](index.md) -- full list of features
