# Getting Started

A quick guide to get Replay Control running on your Pi.

{{< screenshot "home-desktop.png" "Replay Control home page" >}}

## Prerequisites

- A Raspberry Pi running [RePlayOS](https://www.replayos.com/)
- ROM files for the systems you want to play
- A computer, phone, or tablet on the same network as the Pi

## Install

From any computer on the same network, or directly on the Pi:

```bash
curl -fsSL https://raw.githubusercontent.com/lapastillaroja/replay-control/main/install.sh | bash
```

No arguments needed -- the installer auto-discovers your Pi on the network, or installs locally when run on the Pi itself. If no stable release exists yet, it falls back to the latest beta.

If auto-discovery doesn't find your Pi, specify the address directly with `--ip your-pi-ip`. For more install options (SD card, specific version, custom password), see the [Installation guide](install.md).

## First Launch

Open a browser and go to:

```
http://replay.local:8080
```

If `replay.local` does not resolve, use your Pi's IP address instead (e.g., `http://your-pi-ip:8080`).

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
- [Preferences](settings.md) -- region, skins, font size
- [WiFi, NFS & Pi Setup](configuration.md) -- Wi-Fi, hostname, NFS shares, password
- [Feature overview](index.md) -- full list of features
