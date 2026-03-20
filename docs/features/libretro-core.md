# Libretro Core -- Recently Played Viewer

A libretro core (.so) that displays recently played games with box art on the TV screen.

## Overview

The `replay-libretro-core` crate builds a `cdylib` (.so) that RePlayOS loads as a libretro core via the custom frontend. It fetches data from the Replay Control REST API running on localhost and renders a navigable game metadata viewer on the TV.

## Features

- **Recently played games** with box art, display name, system name, and metadata
- **Favorites list** toggled with the Start button
- **Game detail view** showing year, developer, genre, players, rating, and description
- **Box art prefetching** for smooth navigation
- **Adaptive layout** based on CRT (320x240) vs HDMI (720p), detected from `replay.cfg`
- **Gamepad navigation**: D-pad left/right to browse games, up/down to scroll descriptions, B to exit, Start to toggle recents/favorites

## REST API

The core communicates with the companion app via plain REST endpoints (not Leptos server functions) so they have stable, hash-free URLs:

| Endpoint | Method | Returns |
|----------|--------|---------|
| `/api/core/recents` | GET | JSON array of recently played games with box art URLs |
| `/api/core/favorites` | GET | JSON array of favorites with box art URLs |
| `/api/core/game/:system/:filename` | GET | JSON game detail (display name, year, developer, genre, players, rating, description, publisher, region) |

These endpoints are lightweight Axum handlers mounted on the existing server, returning `CoreGameEntry` and `CoreGameDetail` structs.

## Architecture

The core uses `minreq` for HTTP (no async, no TLS -- localhost only) and custom BMP parsing for box art rendering. It implements the standard libretro API callbacks (`retro_init`, `retro_run`, `retro_load_game`, etc.) and renders directly to a framebuffer in RGB565 format.

Target: `cdylib` for aarch64 (Raspberry Pi).

## Key Source Files

| File | Role |
|------|------|
| `replay-libretro-core/src/lib.rs` | Libretro API implementation, game state, input handling |
| `replay-libretro-core/src/http.rs` | REST API client, JSON parsing, box art fetching |
| `replay-libretro-core/src/draw.rs` | Framebuffer rendering, text drawing, image blitting |
| `replay-libretro-core/src/layout.rs` | CRT vs HDMI layout configuration |
| `replay-control-app/src/api/core_api.rs` | Server-side REST endpoints |
