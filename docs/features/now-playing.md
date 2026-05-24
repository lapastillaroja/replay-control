# Now Playing

The companion app shows what is running on the appliance right now: a "Now Playing" pill in the top bar, a hero card on the home page, and a "live" badge on the detail page of the active game. Open browser tabs update automatically when the running game changes.

## What you see

- **Top-bar pill** — present on every page while a game is loaded. Pulsing dot, game name, system, elapsed time. Tap it to jump to the game's detail page.
- **Home hero card** — replaces "Last Played" while a game is active. Quick links to the detail page, favorite toggle, a one-tap **Manual** button that deep-links to the manuals section, and **Stop Game**.
- **Game detail "Now Playing" pill** — the title bar of the active game's detail page shows a pill so you know "this is the one running right now".
- **Elapsed timer** — shows how long the current play session has been active.

The app distinguishes between a running game, the appliance menu, and the appliance not running. During game launches or core transitions, the UI avoids briefly showing stale game information.

## How it works

Detection runs on the appliance and watches the active RePlayOS process. When a real game core is loaded, the app identifies the active ROM, matches it to the library, and sends the result to connected browsers.

The browser receives the current state as part of the first page load, so the top-bar pill and home hero can appear immediately after refresh. Live updates then keep the page current without polling.

## Robustness defenses

- **Launch debounce.** A new game must be observed consistently before it is shown. This avoids flicker while a core is starting.
- **Menu fallback.** If the appliance is running but the active game cannot be identified yet, the UI treats it as the menu instead of showing a stale game.
- **In-core game switches.** Switching from one game to another without leaving the emulator is detected and updates the UI.
- **Service restart handling.** If the appliance process restarts, the elapsed timer resets on the next confirmed game session.

## Performance notes

Detection is lightweight enough to run continuously on a Raspberry Pi. The app avoids sending updates when the active state has not changed, and each browser computes the elapsed timer locally.

## Manual deep link

The home hero card's **Manual** button opens the active game's detail page directly at the manuals section. The page keeps the target in view while cover art and other content finish loading.

## Stop Game

The home hero card includes **Stop Game** while a game is active. This restarts the RePlayOS frontend service, which unloads the running game and returns the TV frontend to the menu.
