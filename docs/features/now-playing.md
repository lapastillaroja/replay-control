# Now Playing

The companion app shows the game currently loaded in RePlayOS. Open browser tabs update automatically when the running game, play state, or disc state changes.

## What you see

- **Player bar** - appears across the app while a game is loaded. It shows cover art, game name, system, play state, elapsed time, and the current disc when RePlayOS reports one. Tap it to jump to the game's detail page.
- **Home page** - continues to show normal library sections while the player bar handles the active game.
- **Game detail "Now Playing" pill** - the title bar of the active game's detail page shows a pill so you know "this is the one running right now".
- **Elapsed timer** - shows how long the current play session has been active.

The app distinguishes between a loaded game, the RePlayOS menu, and RePlayOS being unavailable. During game launches or core transitions, the UI avoids briefly showing stale game information.

## How it works

Replay Control uses the official RePlayOS Net Control API to read the current status. When RePlayOS reports a loaded game, Replay Control matches that game to the local library and sends the result to connected browsers.

Newer RePlayOS builds can report the system halt state. Replay Control treats halt as distinct from pause and shows halt first when both are present. Older RePlayOS 1.7.3 status responses do not include halt state, so the app keeps the previous paused/menu mapping on those builds.

The browser receives the current state as part of the first page load, so the player bar can appear immediately after refresh. Live updates then keep the page current without browser polling.

## Robustness defenses

- **API status gate.** Now Playing only runs when RePlayOS Net Control is connected and authorized.
- **Menu fallback.** If RePlayOS is reachable but no game is loaded, the UI treats it as the menu instead of showing a stale game.
- **In-core game switches.** Switching from one game to another without leaving the emulator is detected and updates the UI.
- **Service restart handling.** If RePlayOS restarts, the elapsed timer resets on the next confirmed game session.

## Performance notes

Polling the RePlayOS API is lightweight enough to run continuously on a Raspberry Pi. The app avoids sending updates when the active state has not changed, and each browser computes the elapsed timer locally.

## Library actions

Favorite and manual actions stay on the game detail page. The player bar stays focused on the active game state and playback controls.
