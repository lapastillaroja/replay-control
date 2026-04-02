# Libretro Core -- Recently Played Viewer

A proof-of-concept libretro core that displays game library data on the TV.

> **Note:** This is a technical experiment, not a production feature.

## Overview

The libretro core runs on the TV via the [RePlayOS](https://www.replayos.com/) frontend. It fetches data from Replay Control's REST API running on localhost and renders a navigable game metadata viewer.

## Features

- **Recently played games** with box art, display name, system name, and metadata
- **Favorites list** toggled with the Start button
- **Game detail view** showing year, developer, genre, players, rating, and description
- **Box art prefetching** for smooth navigation
- **Adaptive layout** for CRT (320x240) and HDMI (720p), detected from the RePlayOS config

## Controls

| Button | Action |
|--------|--------|
| D-pad left/right | Browse games |
| D-pad up/down | Scroll descriptions |
| B | Exit |
| Start | Toggle recents/favorites |
