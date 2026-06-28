# Changelog

Chronological timeline of changes to the Replay Control companion app for RePlayOS.

---

## [Unreleased]

### Changed

- Removing a favorite from a game's detail page no longer asks for confirmation — the star toggles instantly (it's easy to re-add).
- Rebooting the device from **Settings → RePlayOS** now asks for confirmation first, matching Power off.

### Fixed

- Fixed the action result message under the RePlayOS device controls sitting flush against the buttons; it now uses the standard spacing.

---

## [1.0.1]

> Fixes an error when opening Alpha Player movies and music from Recently Played or Favorites.

### Fixed

- Video and audio played through **Alpha Player** is recorded in Recently Played and Favorites just like games, but opening one of those entries used to show a "not found" error. They now open a simple detail page — with the title and a button to play it again on the TV — and keep appearing in your lists.

---

## [1.0.0]

> First stable release. Manage your RePlayOS game library, metadata, and device settings from any phone, tablet, or desktop on your network — nothing to install.
>
> Learn more and see screenshots at <https://lapastillaroja.github.io/replay-control/>, and learn more about RePlayOS, the libretro emulator frontend it pairs with, at <https://www.replayos.com/>.

### Highlights

- **Library** — browse by system with box art, search, and infinite scroll; favorites organized into folders by system, genre, developer, or arcade board; multi-disc M3U handling; inline rename and delete with multi-file safety; and live updates as ROMs change.
- **Systems** — 43 platforms across consoles (NES / Famicom, Super Nintendo, Mega Drive / Genesis, PlayStation, Saturn, Dreamcast, Nintendo 64, PC Engine / TurboGrafx-16, and more), handhelds (Game Boy / Color / Advance, Nintendo DS, Game Gear, Lynx, Neo Geo Pocket), and home computers (Commodore Amiga and 64, MSX, Amstrad CPC, ZX Spectrum, Sharp X68000, DOS, and ScummVM).
- **Arcade** — human-readable names for 15K+ playable games and curated hardware-board grouping (CPS, Neo Geo MVS, Sega / Taito / Namco / Konami systems, Sony ZN, Taito G-Net, and more) across MAME, FBNeo, Naomi / Atomiswave, and Sega ST-V.
- **Metadata & media** — offline databases covering ~34K console, handheld, and computer games plus ~15K arcade games, optional one-click LaunchBox import, per-system box-art, screenshot, and title-screen download with smart matching, and a maintainer CSV metadata export (Metadata → Advanced) for auditing per-ROM coverage.
- **Discover** — recommendations of several kinds that work together: **Because You Love [game]**, genre-based **Rediscover Your Library** spotlights, **Hidden Gems** (highly rated games you haven't played yet), **Related Games** on every detail page, and a rotating **Curated Spotlight** — never repeating a game across sections — plus quick Discover pills, cross-system fuzzy search, developer pages, and series / franchise navigation.
- **Game detail** — box art, screenshots, captures, pinned trailers, on-demand manuals, RetroAchievements status and achievement count, and one-tap launch on the TV.
- **RePlayOS integration** — official Net Control API for launching games, now-playing detection, and reading device settings including skin and log level.
- **Settings & system** — Wi-Fi, NFS, hostname, and password from the browser; skin sync; storage auto-detection (SD / USB / NVMe / NFS) with corruption recovery; and system logs.
- **Blazing fast** — pages render in just 5-6 ms, so browsing feels instant thanks to ["blast processing"](https://segaretro.org/Blast_processing) — and it sips so little memory it leaves your Pi's resources free for the games.
- **Polished experience** — a responsive interface for phone, tablet, and desktop with smooth client-side navigation, swappable skins synced from your device, an installable PWA, and full English / Español / 日本語 translations.

### Thanks

A heartfelt thank-you to everyone who ran the beta releases and took the time to send bug reports, feature ideas, and encouragement. Your support and feedback shaped this 1.0 — it would not be what it is without you.

---

## [0.10.0-beta.6]

> Adds CSV metadata export, speeds up first load with precompressed WASM, and stops incorrectly flagging multi-disc CHD games as unable to earn RetroAchievements.

### Added

- Added a **metadata export** (Metadata → Advanced) that downloads a per-ROM CSV of your library — one row per game with its catalog and LaunchBox metadata, media presence, RetroAchievements details, and classification — for auditing coverage and upstream sources.

### Changed

- Faster first load: the WASM bundle is now served Brotli-precompressed.

### Fixed

- Multi-disc CHD games are no longer flagged as unable to earn RetroAchievements — the disc-format limitation that note described is resolved on the device, so the warning has been removed.

---

## [0.10.0-beta.5]

> Fixes removing games from Recently Played, iOS/WebKit browser Back from game detail pages, and keeps list launch buttons working after returning to search results.

### Fixed

- Fixed removing a game from **Recently Played** not sticking — the deletion now refreshes the cached recents (and recommendations) so the game stays gone instead of reappearing after a reload.
- Fixed iOS/WebKit browser Back from a game detail page forcing a full page reload instead of restoring the search results page in place.
- Fixed compact game-list launch buttons continuing to work after browser Back from a game detail page, including WebKit-family browsers.
- Fixed Favorites rows using an empty overlay-link child that could risk hydration mismatches; they now use the same hidden label structure as search and other game-list rows.

---

## [0.10.0-beta.4]

> Adds the Sony ZN and Taito G-Net arcade boards, and fixes redundant folder nesting when organizing favorites by board and system.

### Added

- Added two arcade hardware boards — **Sony ZN** (ZN-1/ZN-2) and **Taito G-Net** — so games running on them (Street Fighter EX, Bloody Roar 2, Brave Blade, G-Darius, Psyvariar, Ray Crisis, …) are now labelled, grouped, and searchable by board like other arcade hardware.
- Added a **RePlayOS UI log level** indicator on the System Logs page — read live from the TV, with guidance to change it there (SYSTEM > LOG LEVEL) — and gave the **Replay Control log level** control an Error option and a Save & Restart button that applies the change without a manual reboot.
- Added compact launch buttons to game-list rows in search, system/developer/board lists, and Favorites. They launch directly from the list, are disabled for the game already playing, and ask before replacing a running game. On iPhone and iPad these list buttons are hidden — launch from a game's detail page there instead.
- Added the RetroAchievements achievement count to the game detail page, shown next to the trophy for games that have a RetroAchievements set.

### Changed

- Replaced browser confirmation prompts with Replay Control's in-app confirmation dialog across launch, favorite removal, recent removal, saved resource removal, capture deletion, access, and power controls.
- Improved now-playing control feedback so taps highlight only the pressed button, and made clickable game-info metadata use link-colored styling.
- Added a non-persistent language selector to the login page and aligned its styling with the HTTP guidance page selector.

### Fixed

- Fixed games named with Arabic numerals (e.g. `Doom 2`, `Duke Nukem 2`) not picking up descriptions, genres, and release dates when the metadata source spells the title with Roman numerals (`Doom II`). Title matching now treats the two numeral styles as equivalent for metadata, matching the box-art behaviour.
- Fixed organizing favorites by **Board** together with **System** (in either order) nesting console favorites under their system name twice (`<System>/<System>/`); the two levels now collapse to one.
- Fixed a crash that could occur when navigating between pages in the app, caused by the localization context being lost across a client-side navigation.
- Fixed login form fields being cleared when WASM hydration finished after a user had already started typing.

---

## [0.10.0-beta.3]

> Favorites can now be organized by arcade hardware board.

### Added

- Added **By Board** as a way to organize favorites. Arcade favorites are grouped into folders named after their hardware board (CPS-1, CPS-2, Neo Geo MVS, Taito F3, PGM, …), while console favorites fall back to their system name. It works as either the primary or secondary grouping, alongside System, Genre, Players, Rating, and Developer.

### Fixed

- Fixed a crash on the Favorites page when removing a favorite; the list now updates cleanly without reloading.

---

## [0.10.0-beta.2]

> Replay Control now serves the app over local HTTPS by default and introduces session-based app access for normal users and admins.

### Highlights

- **Local HTTPS by default** — the app serves over `https://replay.local:8443` with an auto-generated self-signed certificate (covering `replay.local`, the device hostname, localhost, and LAN IPs); a guidance page on `8080` points devices to it. Regenerating the certificate or changing the hostname now restarts the service and reloads so the new certificate takes effect immediately.
- **Session-based app access** — normal users sign in with the adopted RePlayOS Net Control code and admins with the device password, behind opaque HttpOnly cookies. A one-time first-setup explains the model, and pages, server functions, REST endpoints, SSE, and media routes are now role-enforced (guest browsing removed).
- **Admin session controls** — downgrade to normal user, logout, a configurable admin unlock duration (1h/3h/12h), separate normal-user vs admin sign-in rate limits, and CSRF Origin/Referer checks.
- **More arcade coverage** — board pages and board-aware search for Gaelco 3D, Namco System 10, and Midway Vegas; fixed hardware-board attribution for current-MAME arcade games (Sega Model 1/2, ST-V, Rave Racer, …) and better arcade box-art matching via alternate names.
- **Settings** — Ethernet and Wi-Fi MAC addresses added; low-risk preferences stay interactive while system information refreshes in its own section.

### Added

- Added local HTTPS on port `8443` by default, with an automatically generated self-signed certificate for `replay.local`, the device hostname, localhost, and current LAN IP addresses. Hostname changes made through Replay Control regenerate the certificate automatically, while the Access & Security page shows current certificate coverage and offers manual regeneration for IP/address changes.
- Added an HTTP guidance page on port `8080` that points devices to the HTTPS URL, includes the Replay Control logo, supports English, Spanish, and Japanese based on the preferred language, and validates request hostnames before rendering links.
- Added sign-in with the adopted RePlayOS Net Control code for normal users and device-password sign-in for admin access. Sessions use only an opaque HttpOnly cookie.
- Added a one-time first setup page for device mode. It explains the new normal-user/admin permission model, tells fresh-image users the default password is `replayos`, verifies the current root password, marks `first_setup_done`, and opens an admin session before showing the existing setup checklist.
- Added role enforcement for app pages, server functions, REST endpoints, SSE streams, and media routes. Signed-out sessions can only reach sign-in, setup, static assets, and health/version bootstrap endpoints.
- Added admin session downgrade, logout, login rate limiting, CSRF Origin/Referer checks, unauthorized-state detection when RePlayOS rejects the stored Net Control code, and session invalidation when Replay Control stores a replacement Net Control code.
- Added `--dangerous-disable-https` and `--dangerous-allow-insecure-auth-over-http` debug flags for development and recovery scenarios that explicitly need plain HTTP.
- Added the Ethernet and Wi-Fi MAC addresses to the System section of the Settings page.
- Added arcade board pages and board-aware search for three more arcade hardware boards: Gaelco 3D, Namco System 10, and Midway Vegas.

### Changed

- Updated install, getting-started, and e2e guidance to use `https://replay.local:8443`.
- Kept the libretro core's localhost HTTP API and media routes available on port `8080`, restricted to loopback access, so TV-side browsing continues to work while LAN devices use HTTPS.
- Container and test launch paths now explicitly opt into dangerous HTTP mode when they only expose port `8080`.
- Removed the SD-card install method; install over SSH or directly on the device against a running RePlayOS instance instead.
- Served authenticated library media, manuals, captures, and ROM documents with private cache headers instead of public cache headers.
- Removed guest browsing mode; signed-out sessions can only reach sign-in, first setup, static assets, and health/version bootstrap endpoints.
- The first-run setup checklist is now hidden from normal users on the device; only admins see it (on standalone it still sends users to **Access & Security** for admin unlock before admin-only actions).
- Admin unlock duration now defaults to 1 hour and can be changed from **Access & Security** to 1 hour, 3 hours, or 12 hours; changing it refreshes the current admin session from the time of the change.
- The Settings page keeps low-risk preferences inline and independently interactive while dynamic system information refreshes in its own section.
- HTTPS certificate details (file paths and the LAN address list) on the Access & Security page are now visible to admins only.
- Regenerating the HTTPS certificate, and changing the hostname, now restart Replay Control and reload the page so the new certificate is served and re-accepted immediately. The regenerate button is always shown, disabled for non-admins.
- The admin-to-normal-user downgrade control is now shown disabled, with an explanation, when signed in directly as an admin.
- Sign-in rate limiting now tracks normal-user and admin attempts separately, so failed attempts on one no longer lock out the other.

### Fixed

- Fixed ROM library scans dropping games when a transient filesystem error interrupted a scan, and made symlinked folders follow safely without looping on symlink cycles.
- Fixed sign-in not persisting when HTTPS is disabled, because the session cookie kept its Secure attribute over plain HTTP.
- Fixed the device password change accepting control characters; newlines and similar characters are now rejected before the password is applied.
- Fixed the admin being signed out after changing the device password; the admin session is now re-issued.
- Fixed per-game play time possibly matching a same-named game from another system.
- Fixed HTTPS certificate details not appearing after elevating a normal-user session to admin.
- Fixed the admin unlock duration selector showing the wrong value until a page refresh.
- Fixed status and confirmation messages lacking consistent bottom spacing on settings pages.
- Fixed signed-in visits to `/login` returning to **Access & Security** after local app data was cleared and sign-in completed; they now return to the top page.
- Fixed generated systemd service templates so `REPLAY_EXTRA_ARGS` with multiple flags is split into separate command-line arguments.
- Fixed the Settings and Access pages rendering with hydration mismatches after full refreshes.
- Fixed thumbnail retry processing so permanently failing jobs stop being resubmitted after the attempt cap without recursive worker calls that could exhaust the service stack.
- Fixed the channel selector and **Check for Updates** button in the Updates section rendering at different heights.
- Fixed a console warning logged when clearing metadata or the search index from the Game Library page.
- Fixed the game catalog build to fail fast when any upstream or curated data source is missing or empty, so a partial catalog (for example one missing developer and publisher metadata or Shmups Wiki links) can no longer be shipped.
- Fixed missing arcade box art for games whose cover art exists online under a different name than the one shown (common for some MAME arcade games), by also matching against the alternate names from the other arcade databases.
- Fixed arcade games emulated by current MAME (such as Sega Model 1, Sega Model 2, Sega ST-V, and Rave Racer) showing no hardware board on their detail pages and being absent from board pages and board search.

---

## [0.10.0-beta.1]

> Box art now appears for more games — arcade titles with "&" or "'" in their names, Amiga titles, and sequels named with digits.

### Added

- Recent games can now be removed from the home page. A **×** button appears on hover (always visible on touch devices) for each card in the "Last Played" and "Recently Played" rows. Removal is confirmed before taking effect and is immediately reflected in the UI.

### Changed

- The **Advanced** actions in the Game Library settings page now use consistent button styling: the primary action uses the accent fill, secondary/destructive actions use a bordered style, and the two-step confirmation shows a clearly styled danger button. The action cards are arranged in a 2-column grid on wider screens so labels no longer collapse into narrow columns.
- The home **Recently Played** row, the **Recently Added** favorites row, and the **More like this** and **More on this board** rows on a game's page now show up to 12 games (previously 8–10), filling out the rows on larger screens.

### Fixed

- Fixed missing box art for arcade games whose names contain an ampersand or apostrophe — for example **Dungeons & Dragons: Shadow over Mystara**, **Street Fighter Alpha: Warriors' Dreams**, and other CPS2 titles. The game info was already correct, but the photos were missing; the underlying game names are now read correctly so the artwork matches.
- Fixed Amiga (WHDLoad) game names being cut off at an ampersand — **4th & Inches** no longer shows as just "4th" — and these games now line up with their library entry instead of being listed twice.
- Fixed box art not appearing when a game's name uses a digit where the artwork uses a Roman numeral (or the reverse), such as MS-DOS **Doom 2** now matching **Doom II** and **Arkanoid 2** matching **Arkanoid II**.
- Fixed console game metadata imported from the games database (titles, publisher, developer, genres) being cut off at an ampersand or apostrophe — genres such as **Action & Adventure** and titles or studios containing "&" now read in full instead of being run together.

---

## [0.9.0]

> A richer game detail page with saved resources, user captures, and clearer recommendations.

### Highlights

- **Game detail pages now organize everything you need around the game itself.** Game info has a denser summary, expandable descriptions, and a full metadata table; screenshots and user captures share one gallery and lightbox; and related games are grouped under a clear **Recommendations** section.
- **Resources are now first-class.** Manuals, guides, links, and videos can be saved to a game, while suggested catalog resources stay separate and shrink back once you have your own saved items. Suggested manuals download as manuals, videos can be searched or pasted, duplicate links are rejected consistently, and saved resources update immediately in the UI.
- **User captures can be managed from the detail page.** Captures appear alongside provided screenshots with matching thumbnail sizing, update live as new screenshots are taken, and can be deleted with confirmation from the gallery or lightbox.

### Changed

- The Settings metadata page is now presented as **Game Library**, with the new `/settings/game-library` URL while the old metadata URL remains available for compatibility.
- Back buttons, game-title wrapping, resource rows, pending save indicators, and expanded video embeds were aligned with the app's shared control styling.
- The detail page now keeps long game titles readable at the normal title size, wrapping up to three lines instead of shrinking text.

### Fixed

- Fixed saved resource links with superficial URL differences, such as trailing slashes or casing, being stored as hidden duplicates that could reappear after deletion.
- Fixed hydration warnings caused by resource actions reading Leptos resources outside Suspense during client-side interaction.
- Fixed GDI and multi-file disc sizes being shown as `0 KB` when the main descriptor file was small.

### Performance

- The Game Library page's per-system coverage stats are recomputed faster during library scans — each system's counts are now gathered in a single database pass instead of many repeated queries (measured ~1.5× faster on the device).

---

## [0.8.0]

> RetroAchievements library flagging, Amiga identification, and cleaner game names.

### Highlights

- **RetroAchievements games are flagged in your library — matched precisely by content.** Games with a RetroAchievements achievement set show a 🏆 indicator on their detail page, and a **Has achievements** filter lets you browse only those games (in global search and on the system, developer, and board lists). Matching is by content rather than title, so the flag is correct for the exact dump you own and reaches cartridge, arcade, **and disc** systems — PlayStation, Sega CD, Sega Saturn, Dreamcast, 3DO, and more.
- **Disc games are recognized by their boot content.** A disc is identified by the same fingerprint RetroAchievements uses, so the specific version you have is flagged — or left unflagged when it genuinely has no achievement set, instead of guessing.
- **RetroAchievements clearly shows when a game can't be earned on RePlay.** A flagged game still shows the 🏆, but both the game detail page and the per-system coverage view now add a note when the system's emulator doesn't support achievements (PlayStation, PC Engine CD, arcade MAME, and others). Compressed disc images (`.chd`) are also flagged as unavailable for now, pending a RePlayOS fix. This keeps expectations clear instead of implying every flagged game is earnable today.
- **Amiga games are now identified in your library.** Commodore Amiga and Amiga CD32 titles are recognized across the common naming conventions (WHDLoad, ADF, IPF), with genre, developer, and year filled in where available. Titles are cleaned up — "SuperFrog" instead of "SuperFrog_v1.1_0485 (Europe)", and "Batman - The Movie" instead of "Batman - The Movie v1.0 [cr QTX][h BTL]" — while regional variants you own as distinct games keep a clean region suffix so they stay distinguishable.
- **Per-system pages show RetroAchievements coverage.** Each system's metadata view shows how many of your games have an achievement set, and clearly notes the systems RetroAchievements doesn't support.
- **Settings system info refreshes live.** The System section in Settings now updates every second while you have the page open — CPU temperature, available RAM, network IP addresses, disk space, and OS uptime all stay current in real time.

### Fixed

- **More regional game variants now show a clean region name.** Games named in the TOSEC convention (Amiga, X68000, and others) from less-common regions — Poland, Denmark, Finland, Canada, Norway, Czechia, and many more — now display a readable region (e.g. "Game (Poland)") instead of a raw two-letter code like "(PL)". The major regions (USA, UK, Germany, …) are unchanged.
- **Game names are consistent across the app.** A game's display name is resolved once during the library scan and reused everywhere — home, recently played, and lists now show the same clean name. Multi-disc playlists (`.m3u`) no longer pick up a stray disc label, and multi-disk TOSEC playlists with distinct editions each get their own name instead of both resolving to the disk-1 label.
- **Library scans identify games more reliably.** Achievement/content identification is now scheduled before per-system metadata enrichment, so a transient enrichment hiccup no longer skips identifying that system's games.
- **Per-system metadata coverage counts content-matched games as verified.** The "Verified" coverage row now includes games identified by their RetroAchievements content hash (discs), not just No-Intro CRC matches (cartridges).

---

## [0.7.0]

### Highlights

- **Games now show which arcade board they ran on.** Arcade titles display their original hardware — like "CPS-2 (Capcom)", "Neo Geo MVS (SNK)", or "F3 System (Taito)" — on the game detail page. The board links to its page, and a "More on this board" row suggests other games on the same hardware.
- **Browse every game on an arcade board.** A new board page lists all the games in your library that share a board, with the same system and content filters as the developer pages.
- **Search understands arcade boards.** Typing a board name or shorthand — for example "cps", "neo geo", "naomi", or "f3" — surfaces a "Games on …" preview plus a list of other matching boards, each linking straight to its board page.
- **Arcade boards turn up in your recommendations.** The home page mixes board shortcuts into the Discover pills (e.g. "More CPS-2 (Capcom)") and occasionally spotlights a board you own a lot of ("Games on Neo Geo MVS").
- **Redesigned game cards.** Game cards across the app now lead with a short system tag in the accent color and show larger box art, making your library quicker to scan.
- **Settings now shows device hardware.** The System section adds your Raspberry Pi model, CPU temperature, and available memory alongside the existing storage and network details (shown when running on the device).

### Changed

- **RePlayOS 1.7.4 is now the minimum supported version**, following its updated Net Control configuration interface. RePlayOS settings changes are applied in a single request, and an older RePlayOS is detected and flagged with a prompt to update.
- **The Now Playing bar shows the system shorthand.** The bottom row now uses the short system tag in the accent color (matching game cards) instead of the full system name.

### Fixed

- The Now Playing title stays on a single line in portrait orientation.
- Restored the page back button's border.
- Tidied control sizing and alignment across search, the favorite star, and buttons.

---

## [0.6.0]

### Highlights

- **Save states are available from the Now Playing bar.** The "More" panel now lets you pick slots 1-18, save/load through the RePlayOS API, and see compact real slot status from `.sst` files with recent saves updating from "just now" to minute-based labels.
- **RePlayOS device actions live in Settings.** The RePlayOS settings page now includes on-screen messages, current-game restart, device reboot/power-off, and kiosk mode controls, with each action showing its own result message inline.
- **Initial app loading is visible and localized.** Slow WASM startup now shows a small translated loading strip instead of a blank or frozen-looking page.

### Changed

- The RePlayOS settings page keeps on-screen message text after sending, so repeated messages can be resent or edited; a separate Clear message button empties the field.

### Fixed

- Fixed custom cover art in the Now Playing bar by centralizing effective box-art resolution with the game-detail path.
- Fixed startup scans showing the last scanned system name during the media-stat refresh step; the banner now switches to "Updating media stats..." for that final pass.
- Fixed activity banners so scan/import/update messages stay visible while scrolling and stack below the Now Playing bar when a game is active.

---

## [0.5.0]

### Highlights

- **Replay Control now uses the official RePlayOS API for TV integration.** Launch on TV goes through RePlayOS Net Control instead of writing autostart files, editing `replay.cfg`, or restarting `replay.service`, eliminating the storage-remount path that could downgrade NFS/USB/NVMe libraries back to SD.
- **Net Control setup and status are first-class.** Settings can guide device users through enabling RePlayOS Net Control, restarting the TV interface, reading the control code, and storing the verified code in Replay Control; launch now reports connection/control-code problems directly instead of falling back to unsafe restart behavior.
- **Now Playing is backed by RePlayOS status.** The home and detail surfaces now reflect the TV's actual running game, including arcade clones, multi-disc games, ScummVM titles, elapsed play time, paused/playing state, optional halted state on newer RePlayOS builds, and current-disc labels like "Disc 2/4".
- **On-TV controls are available from Replay Control.** The redesigned Now Playing UI adds screenshot, volume, mute, halt, and reset controls using the RePlayOS API while removing the old Stop Game action that depended on frontend restarts.
- **Storage and metadata activity are safer and clearer.** Launch no longer restarts the frontend, NFS outage recovery avoids probing stale hard-mounted shares, and long media-stat refreshes now show their own progress label instead of looking like the previous system scan is stuck.

### Changed

- Launch is refused while Replay Control is already doing storage-mutating work, but it no longer opens a `FrontendRestart` quiesce window because successful API launches do not restart `replay.service`.
- Now Playing offers no stop/unload action (RePlayOS exposes no stop API); active-game surfaces deep-link to details/manuals, and game control happens through the new screenshot/volume/halt/reset buttons instead of frontend restarts.
- The legacy autostart launch helper script and restart-based launch process have been removed from the active release path.

### Fixed

- Fixed Replay Control launch on NFS by avoiding the RePlayOS service restart/remount path entirely. On-device validation launched a Master System ROM from NFS through Net Control while `replay.service` kept the same PID/start time and `system_storage` remained `nfs`.
- Fixed NFS outage recovery during storage fallback detection. Replay Control no longer probes a stale hard-mounted NFS share before accepting the configured-storage error/fallback state, avoiding hangs when the NFS server is unreachable.
- Fixed stale stop UI/server-function artifacts by removing the component, endpoint registration, i18n keys, CSS classes, and stop-specific regression test hooks.

---

## [0.4.0]

Stable release rolling up the 0.4.0-beta.1 → beta.16 series. The most important user-facing changes:

### Highlights

- **Now Playing, everywhere.** While a game is running on the appliance, a live badge in the top bar shows its name, system, and elapsed play time on every page; the home page gets a hero card for the active game and the game's own detail page gets a "Now Playing" pill. Tap to jump straight to it, deep-link to its manual, or **Stop Game** to return to the menu. Detection is accurate across the hard cases — arcade clones show the exact running variant, and ScummVM, Neo Geo, and MAME titles are identified correctly instead of as a menu or an internal data file.
- **Run Replay Control off the device.** A new standalone mode (`--storage-path /path/to/roms`) makes managing a library from a desktop or laptop a first-class deployment. Browsing, favorites, search, metadata, and recommendations work the same; device-only features (Wi-Fi, NFS, RetroAchievements, launch on TV, …) are clearly marked unavailable instead of silently writing to a folder the OS doesn't own.
- **Faster, safer library scans — especially on NFS.** Scans now show the file listing and metadata first and run CRC identity matching in the background, so large libraries stay responsive (the 95k-ROM NFS test library went from a ~10-minute forced rebuild to a ~2.5-minute responsive foreground pass). Unchanged systems skip rework on restart via a storage-safe modification-time fast path, rebuilds are resumable and safe to interrupt (a dropped mount or power loss no longer wipes your library), and a rescan now reflects ROMs you deleted on disk.
- **Much better metadata, and one button to refresh it.** A redesigned pipeline stores external metadata host-global, so ROMs added after an import get enriched automatically, and a single **Refresh metadata** action downloads, parses, and re-enriches with live progress. Coverage improved across release dates, descriptions, developers, genres, ratings, and player counts — including arcade clones, alternate regional titles, and filename-only matches — and arcade names now follow each system's upstream curation (FBNeo's "Galaga '88" on `arcade_fbneo`, MAME's name on `arcade_mame`).
- **Richer game detail pages.** Box art, title screen, screenshots, and your own captures share one swipeable lightbox; precise release dates ("Aug 31, 2000") show when known; and pages link out to GameFAQs and Shmups Wiki strategy guides + Video Index walkthroughs (with version/label variants deep-linking to the right section). You can save manuals offline or add your own by URL or upload.
- **Community-curated metadata.** A new bundled source lets anyone contribute descriptions, art, manuals, videos, and guides for games no upstream source covers — one JSON file per system, no code changes — so titles like the AmigaVision distribution finally show real details.
- **SEGA Titan Video (ST-V) support.** ROMs dropped into `arcade_stv/` now appear as a full system with display name, icon, Megabit ROM sizes, MAME / LaunchBox / Wikidata metadata, box art, manuals, and Now Playing detection.
- **More reliable storage, updates, and recovery.** A central per-storage library database keyed to each drive's filesystem id means re-plugging a USB keeps every cached row — no rescan; a clobbered or torn-write database recovers automatically (with a one-click Reset for your saved data) instead of crash-looping; auto-update swaps the bundled game catalog atomically and no longer traps browsers in a reload loop; and slow NFS mounts wait gracefully instead of failing startup.
- **Snappier under load.** Async connection pools and async subprocess/filesystem calls roughly doubled homepage throughput, and a longer response cache keeps the recommendations and favorites carousels warm across navigation.

For the complete per-release detail, see the `0.4.0-beta.*` entries below.

---

## [0.4.0-beta.16]

### Fixed

- Fixed released builds shipping an outdated game catalog. The release was rebuilding the catalog from a stale cached copy of its source data, so recent Shmups Wiki Video Index links and Wikidata series updates (added in 0.4.0-beta.14 and 0.4.0-beta.15) were never actually included and didn't reach devices. Releases now build the catalog from the current committed data, so those Video Index links and series relationships show up after upgrading.

### Changed

- Refreshed the bundled Shmups Wiki index and Wikidata series snapshots to their latest revisions, so game-detail pages reflect current Video Index / strategy-guide links and series relationships (including the version- and label-variant section deep links).
- Each browser tab now holds a single live-updates connection for skin, activity, and Now Playing changes instead of one connection per topic, reducing the number of open connections to the device and improving reliability across reconnects (for example after an auto-update restart).

---

## [0.4.0-beta.15]

### Fixed

- Fixed regional names and label variants that redirect to a parent game on Shmups Wiki linking to the top of the parent's Video Index instead of their own section. Games like *DoDonPachi DaiOuJou Black Label*, *DoDonPachi DaiOuJou Tamashii*, *DoDonPachi III*, *Mushihime-Sama Futari Black Label*, *Deathsmiles Mega Black Label*, *Fire Shark*, *Tengai*, *Sorcer Striker*, and *Truxton II* now deep-link to their exact section of the Video Index when the section can be matched unambiguously.

### Acknowledgments

- Thanks again to [@f8less](https://github.com/f8less) for continued Shmups Wiki testing and reporting.

---

## [0.4.0-beta.14]

### Highlights

- **More games now show their Shmups Wiki Video Index, and version variants link to the exact section.** Many shmups whose wiki page is a video walkthrough without a full article — for example *Shikigami no Shiro*, *Pulstar*, *19XX: The War Against Destiny*, and *Zero Gunner 2* — now show a Video Index link on their detail page. Version and regional variants now deep-link to their specific section of the parent's Video Index instead of the top (e.g. *DoDonPachi Dai-Fukkatsu Ver 1.0 / 1.5 / 1.51* each jump to the matching Version section), and an arcade clone now shows its *own* version's section rather than its parent set's. Variants that have no wiki page of their own are matched to the parent game through Shmups Wiki redirects (e.g. *Ibara Kuro Black Label* → *Ibara*, *Mushihime-Sama Futari Ver 1.5* → *Mushihimesama Futari*).

### Fixed

- Fixed shmups whose Shmups Wiki entry is a Video Index page with no standalone game article never showing a Video Index link. These are now indexed directly, so games like *Shikigami no Shiro*, *Pulstar*, and *19XX* link to their walkthrough.
- Fixed version/label variants linking to the top of a parent's multi-section Video Index instead of their own section. Variants now deep-link to the right section (e.g. *Version 1.5*, *Arrange A*) when it can be matched unambiguously, and fall back to the page top otherwise.
- Fixed an arcade clone inheriting its parent set's Video Index section instead of its own — for example *DoDonPachi Dai-Fukkatsu Ver 1.0* previously linked to the *Version 1.5* section and now links to *Version 1.0*.

### Acknowledgments

- Thanks to [@f8less](https://github.com/f8less) for help improving the Shmups Wiki integration — reporting the missing Video Index links and creating the variant→parent redirect pages on shmups.wiki.

---

## [0.4.0-beta.13]

### Highlights

- **Running Replay Control off the Pi is now a first-class deployment.** Pointing the app at a ROM folder with `--storage-path /path/to/roms` is a supported standalone mode for managing a library from a desktop or laptop, distinct from running on the RePlayOS device. Library browsing, favorites, search, metadata, and recommendations work the same in both modes; device-only features (Wi-Fi, NFS, hostname, password change, frontend restart, system reboot, launch on TV, RetroAchievements) are hidden or marked as unavailable when running off-device, and direct API calls to those features return a clear "Save skipped (standalone mode)" response instead of silently writing to a folder the OS does not own.
- **Community-curated metadata is a new bundled catalog source.** Per-system JSON files under `data/community/` ship descriptions, box art, screenshots, manuals, videos, and strategy-guide links for games no upstream source covers — for example the **AmigaVision** Amiga distribution now shows a real description, year, developer, publisher, and genre on its detail page. Adding metadata for new entries is a JSON edit and a PR; no Rust changes are required. The catalog version stamp covers these entries, so existing installs pick them up automatically on the next boot after upgrading.
- **SEGA Titan Video (ST-V) is now a recognized arcade system.** RePlayOS 1.7.1 added ST-V emulation (Saturn-derived hardware, dispatched to the Saturn core); ROMs dropped into `arcade_stv/` now show up in the system grid with a display name, abbreviation, icon, ROM-size in Megabit, MAME-curated metadata (year, manufacturer, players, genre, status), LaunchBox metadata (overview, ratings, videos, Wikipedia), Wikidata series links, libretro-thumbnails box art, retrokit-arcade manuals where available, and Now Playing detection when the title is running.

### Added

- A user-triggered storage refresh in standalone mode now detects when the `--storage-path` folder has gone missing (USB unplug, network share dropping) and surfaces it through the same waiting/banner UI the device uses for storage problems, instead of letting subsequent ROM reads fail with raw filesystem errors.
- The RetroAchievements menu entry on the Settings page now shows the same "Available only on the RePlayOS device" hint that Wi-Fi, NFS, Hostname, and Change Password already show in standalone mode, so the disabled state is no longer silent.
- AmigaVision now has a curated description, year, developer, publisher, and genre on its detail page when its boot file (`AmigaVision.hdf` / `AmigaVision.adf`) is present in the Amiga ROM folder.
- Anyone can contribute metadata for ROMs not covered by the bundled sources (No-Intro / TheGamesDB / MAME / LaunchBox). One JSON file per system lives under `data/community/`, with per-entry support for title, description (optionally polyglot with `en` required), year, developer, publisher, genre, players, cooperative flag, box art / title / screenshot URLs, manuals, videos, strategy guides, and video indexes. Submission flow and schema are documented in [docs/contributing/community-metadata.md](docs/contributing/community-metadata.md).

### Changed

- Empty or whitespace-only `replay.cfg` files are now refused at every read site — including the config-file watcher, save-side read-modify-write paths, and the RetroAchievements settings page read — instead of being adopted as a blank config that silently defaulted storage to SD and cleared Wi-Fi/NFS/RetroAchievements fields. Closes a remaining window in the [0.4.0-beta.12] fix where the empty-file check lived on only one read path.
- Background storage detection has been simplified to rely entirely on kernel-driven events (`replay.cfg` file watcher + mount-table watcher). The 10-second/60-second belt-and-suspenders poll has been removed; mount-table events now do a full config + storage reload so the boot-recovery case (booted with no `replay.cfg`, then the SD appears) still works without the poll fallback.
- LaunchBox import now field-level merges duplicate entries for the same game on the same system. LaunchBox dual-lists many arcade titles under both a specialty platform tag (`Sega ST-V`, `Sega Naomi`, `Microsoft MSX2`) and a generic tag (`Arcade`, `Microsoft MSX`), where the latter typically carries the richer fields (VideoURL, Overview, Wikipedia link). Previously whichever row appeared last in the XML overwrote the other; the importer now merges field-by-field with non-empty values winning, so the per-game data is the union of both rows regardless of XML order.
- LaunchBox auto-import is also gated on a fingerprint of the platform-map. Adding a new system that participates in LaunchBox import (or changing an existing system's `launchbox_platforms`) now invalidates the gate and re-parses the cached XML on the next boot, instead of silently skipping the reparse because the upstream XML hash hadn't changed.

### Fixed

- Fixed the Shmups Wiki Video Index link being missing on game detail pages for arcade ROMs whose wiki page is a release variant of a parent that hosts the shared videos. For example, `ddpdfk` (DoDonPachi Dai-Fukkatsu Ver 1.5) now links to the same `/Video Index` as `dfkbl` (Black Label), instead of showing only a strategy guide. The bundled wiki index now records when a variant inherits its Video Index from a parent page, covering `Ver X.Y`, `vX.Y`, `Arrange [A]`, `exA Label`, `Black Label`, and `… Edition` suffixes. Sequels and series-overview pages are intentionally excluded so unrelated games' videos are never linked.
- Fixed partial RetroAchievements credentials being accepted in standalone mode. The all-or-nothing rule (both username and password, or both empty to clear) now applies at the API entry point in both modes, instead of only being enforced inside the device-only write path that standalone mode skips.
- Fixed the RetroAchievements settings page erroring out when opened during a `replay.cfg` rewrite window. The page now falls back to the in-memory last-known-good values whenever the on-disk file is missing, empty, or mid-rewrite, instead of surfacing a raw read error.
- Fixed Wikidata series enrichment for SEGA ST-V games. The Wikidata extract script was mapping QID Q1067380 (Sega Titan Video) to `arcade_fbneo`, so series rows like *Decathlete*, *Virtua Fighter Kids/Remix*, and *Puyo Puyo Sun* landed on the FBNeo system and never matched ROMs in `arcade_stv`. The script now routes those entries to `arcade_stv` and the committed snapshot in `data/wikidata/series.json` has been backfilled with the same correction.

---

## [0.4.0-beta.12]

### Highlights

- **RetroAchievements credentials are configurable from Replay Control.** Settings now includes a RetroAchievements page that writes the RePlayOS username/password keys without ever returning the saved password to the browser. Applying RetroAchievements, Wi-Fi, and NFS config changes now stops the TV frontend, writes `replay.cfg`, then starts the frontend again so the behavior matches how RePlayOS consumes those settings.
- **Running games can be stopped from Replay Control.** The Now Playing hero and the active game's detail page now expose **Stop Game**, which restarts the RePlayOS frontend service to unload the current game and return the TV frontend to the menu.
- **Search is more consistent across global and system pages.** Global search and per-system game lists now share the same controls, 400 ms debounce, `q` query parameter, and ranked search behavior, with alias matches available in both places and a system-scoped Random Game button on system pages.
- **Now Playing identifies more games correctly.** Arcade clones now show the exact variant that is running instead of the parent set — *The Simpsons (2 Players)* no longer appears as the 4-player parent — MAME games no longer surface a stray "roms" entry, ScummVM games show the real game instead of one of their internal data files, and Neo Geo games (which run on the FBNeo core) are detected instead of being shown as the menu.

### Added

- Settings now includes a RetroAchievements page for configuring `rcheevos_username` and `rcheevos_password` in the RePlayOS config. Credentials are all-or-nothing, clearing removes both fields, and the stored password is write-only from the UI.
- The Now Playing hero and active game detail page now include a **Stop Game** action for unloading the current game via a RePlayOS frontend restart.
- Per-system game lists now include a Random Game button that jumps to a random playable game from that system.
- A second "Video index on Shmups Wiki" link appears on the game detail page for games whose wiki page has a curated Video Index sub-page (members of Shmups Wiki's Category:Video Index), such as DoDonPachi DaiOuJou.

### Changed

- Wi-Fi, NFS, and RetroAchievements config saves now apply by stopping `replay.service`, writing `replay.cfg`, and starting `replay.service` instead of leaving the user to perform a separate reboot/restart step.
- Global search and per-system game lists now share the same search UI, 400 ms debounce, `q` query parameter, and core ranked search path. The old per-system `search` query parameter is no longer supported.
- The startup reconcile that re-runs per-system enrichment when bundled inputs change is now a single composite stamp covering every bundled enrichment input (the catalog database hash and the Shmups Wiki page index + matcher version). Any input changing invalidates the per-storage stamp, so upgrades pick up new manual links, new Video Index entries, and matcher improvements automatically without a manual rescan.

### Fixed

- Fixed Shmups Wiki linking missing arcade dual-name base titles. Lookups now also try the parts on each side of ` - ` (subtitle, e.g. `darius gaiden - silver hawk`) and ` / ` (dual-region name, e.g. `soukyugurentai / terra diver`) after a direct miss, so MAME/FBNeo entries like Darius Gaiden and Soukyugurentai resolve to their wiki pages. Real titles with hyphens (`R-Type`) are preserved because only space-wrapped separators trigger the fallback.
- Fixed whitespace-only global searches returning the full library instead of the empty search state.
- Fixed Now Playing reporting the wrong game in several cases. Arcade clones could resolve to their parent set — a different game (e.g. `simpsons2p` shown as `simpsons`); MAME's combined search-path string was mistaken for a ROM and shown as "roms"; ScummVM games showed an internal data file (e.g. `SPEECH2.CLU`) or the raw folder; and Neo Geo games were not detected at all (shown as the menu). The detector now drops the bare parent short-name, ignores the search-path string, picks the ScummVM content file and matches it to the library title, and recognises Neo Geo on the FBNeo core.
- Fixed Launch on TV failing when it was triggered before the page finished loading.
- Fixed Stop Game leaving the boot autostart trigger in place; stopping a game now clears it so the game does not relaunch on the next reboot.
- Fixed config saves so a failed write can no longer truncate or recreate `replay.cfg`: the file is written atomically, and an unexpectedly empty or missing config is refused rather than overwritten.

---

## [0.4.0-beta.11](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.11) - 2026-05-16

### Highlights

- **Large library builds stay responsive while the scan runs.** Replay Control now writes the visible file listing and metadata first, then runs CRC identity matching in the background. Big libraries benefit on any storage, with the biggest gains on slow or high-latency storage such as NFS. On the 95,495-ROM NFS test library, the foreground scan/enrichment pass completed in ~145-148 s in earlier validation, compared with beta.9's 194 s rescan and 636 s forced rebuild path, while the UI stayed usable during the work.
- **Stable libraries restart faster without trusting unsafe storage timestamps.** Startup still checks every system folder, including nested ROM folders, but unchanged systems now skip database rewrites and metadata enrichment after the file check. The skip path is gated by a per-storage mtime reliability probe, so Replay Control falls back to normal reconciliation if the active storage cannot safely use file modification times. A normal manual rescan still refreshes metadata when requested.
- **Now Playing stays on the actually-running game.** Opening the ReplayOS overlay menu and browsing other sections (notably Dreamcast and PlayStation) no longer flips Now Playing to whichever game is highlighted there. The detector now filters heap candidates by the systems the loaded core can run, keeps the previously-tracked game locked when it is still in the heap, prefers the path with the highest occurrence count, and drops prefix-truncation artefacts so games like gunlock consistently show their name, box art, and a working detail link.
- **Game detail pages link out to external guides.** A "Look up on GameFAQs" link opens a GameFAQs title search for the current game, and a "Strategy guide on Shmups Wiki" link goes straight to the game's wiki page when the title appears in the bundled monthly-refreshed index. Regional-variant titles resolve via the library alias table, so games like Gunlock land on the right RayForce page.

### Added

- Game detail pages now show a "Look up on GameFAQs" link that opens a GameFAQs title search for the current game. Hidden for utility entries (video/audio playback).
- Game detail pages now also show a "Strategy guide on Shmups Wiki" link when the game appears in the bundled Shmups Wiki page index. The link goes directly to the game's wiki page (no search step). Regional-variant titles resolve via the library alias table, so games like Gunlock correctly link to the RayForce wiki page. The index is refreshed monthly from the wiki via CI.

### Changed

- Rebuild/rescan is split into foreground discovery/enrichment and a deferred identity phase. Normal rescans still reuse valid cached CRC identity; forced rebuilds mark hash-eligible rows for background re-identification instead of blocking the visible library on every ROM byte read.
- The identity phase is resumable and bounded. Rows move through explicit identity states, interrupted work is picked up later, hash matching defaults to two workers for every storage class, and progress advances after each 200-ROM mini-batch.
- Per-system library writes now reconcile with durable scan tokens. Current rows are upserted in bounded chunks, stale rows are deleted only after finalization, and unchanged ROM resources survive rescans.
- Enrichment writes are now chunked where safe, and game-detail resources are staged before live replacement so interrupted scans do not leave descriptions or resource links empty.
- First-run metadata and thumbnail-source downloads no longer block the first library scan. The library appears from local discovery first, then optional metadata and artwork fill in as background work completes.
- Thumbnail downloads now use a durable per-storage queue with box art first, then title screens, then screenshots. Temporary GitHub throttling and service errors are retried with bounded backoff instead of creating request bursts.
- Metadata-page library summary, release-date/publisher/media coverage, romset composition, downloaded artwork totals by type, and per-system coverage now read from rebuildable stats in `library.db`; upgraded libraries backfill coverage on open and refresh media totals after scan/rebuild/thumbnail update maintenance, and the old app-local metadata page snapshot cache is removed.
- Startup scans now store a per-system file fingerprint after complete discovery/enrichment/identity work. On later boots, unchanged systems skip discovery writes and enrichment after the recursive file check, while changed or incomplete systems are reconciled normally. The fast path is enabled only after the active storage passes a signature-scoped mtime reliability probe; probe absence, probe failure, or a storage signature change forces the safer normal reconcile path.

### Fixed

- Fixed a home-page hydration mismatch when refreshing while a game is running. The top-bar pill and home Now Playing card now SSR from the same bootstrapped state that hydration adopts, and the home branch keeps a stable shape while its detail data resolves.
- Fixed Launch on TV failing on Pis with large libraries or slow storage. The post-launch watcher now polls the replay binary's actual state instead of using fixed 5s/10s timers, so launches still succeed when the binary takes longer than 5 seconds to read the autostart file (observed up to ~7 seconds on 100k-ROM Pi 5 setups). The recovery restart now only fires when the binary is genuinely hung, removing the second screen flash on the common menu-recovered path.
- Fixed Now Playing flipping to the wrong game when navigating the ReplayOS overlay menu while playing. Browsing some sections (notably Dreamcast and PlayStation) leaks the highlighted ROM path into the running process's heap, which the detector previously mistook for the active game. The detector now filters by which systems the loaded core can run, keeps the previously-tracked game locked when it is still present in the heap, and prefers the path with the highest occurrence count.
- Fixed games occasionally showing as a plain filename with no box art and a dead detail link (e.g. gunlock). The heap walk could pick up a truncated copy of the ROM path that did not match the library row; truncated variants are now dropped when the full path is also present.

---

## [0.4.0-beta.10](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.10) - 2026-05-15

### Highlights

- **Game pages have better resource links.** Replay Control now bundles catalog/manual links, surfaces metadata video suggestions, lets you save manuals for offline use, and lets you add your own PDF/text manuals by URL or upload.
- **Game browsing feels more predictable.** Random Game now picks from actual library rows, related/series games sort by release date when known, and stale browser state after random navigation is fixed.
- **Large libraries stay accurate after storage changes.** System summaries are derived from `library.db` metadata instead of stale in-memory cache, and background maintenance uses actual active systems from the library.
- **Manual links are cleaner.** Broken legacy manual URL families are filtered or rewritten so game pages do not show suggestions that fail immediately.
- **Release packaging is more reliable.** Built-in series data now comes from a committed Wikidata snapshot instead of live public queries during release builds.
- **Upgrades are less likely to hit stale browser files.** Non-fingerprinted WebAssembly helper snippets now revalidate after deploys, preventing startup failures caused by old cached helper scripts.

### Added

- Bundled catalog now includes manual resource links from MiSTer Manual Downloader and Retrokit. They are matched during library enrichment and exposed on game detail pages without runtime source-index downloads.
- Saved manuals are tracked in `user_data.db` and stored under `.replay-control/manuals`. Downloads are validated as PDF or text before they are added, and duplicate suggestions are hidden after a manual is saved.
- User-provided manuals can be added from game detail pages by pasting a PDF/text URL or uploading a PDF/text file up to 64 MiB. Uploaded manuals use the same local storage and delete flow as downloaded manuals.
- LaunchBox video URLs are imported as provider resources and shown as metadata video suggestions that can be pinned like search results.
- A monthly GitHub Actions workflow refreshes the committed Wikidata series snapshot and opens a review PR when the data changes.

### Changed

- External metadata storage now uses generic provider tables instead of LaunchBox-only table names, leaving the metadata pipeline ready for additional sources without changing the game-detail read path.
- Game-detail description/publisher and manual/video suggestions are now copied into per-storage library tables during enrichment. Request-time game pages stay on `library.db`; provider and catalog databases are not queried on page load.
- Wikidata series data is now built from committed `data/wikidata/series.json` during release builds. Local maintainers can still refresh it with the extractor script, but CI no longer falls back to live SPARQL queries when the snapshot is missing.
- System summaries are now a derived read view over the static `SYSTEMS` catalog plus `game_library_meta` counts, not an in-memory library cache. System-list endpoints still return every visible system, while info and metadata coverage paths read `game_library_meta` directly when they only need counts.
- Background maintenance paths now use a distinct `active_systems` helper backed by actual `game_library` rows. Rebuild/rescan discovery still walks every `visible_systems()` entry so systems that were previously empty are not missed.
- The game detail video section now presents saved videos, suggested metadata videos, URL entry, and online search as separate compact groups so the source labels do not dominate the card layout.

### Fixed

- Wikidata series enrichment now chunks queries by series QID to avoid long WDQS requests timing out.
- TGDB alias resolution now uses catalog IDs, improving matches for games whose external names differ from library filenames.
- Random Game now picks directly from actual `game_library` rows and navigates through the Leptos router, fixing stale browser-back state after jumping to a random game's detail page.
- Related games now sort by release date when known, making series and franchise lists read in a more natural order.
- Stale manual URL families from older catalog data are filtered/rewritten so broken legacy manual suggestions do not appear on game pages.
- Horizontal scrolling of related-game rows on iOS Safari no longer snaps back to the start while a game is running. Elapsed-time displays now update once per minute instead of every second, avoiding layout invalidations during momentum scroll.
- Browsers no longer keep stale wasm-bindgen inline snippet files across upgrades. Fingerprinted JS/WASM remains long-cacheable, while non-fingerprinted snippet files now revalidate so a freshly deployed app does not fail during startup with a WASM import error.

---

## [0.4.0-beta.9](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.9) - 2026-05-07

### Highlights

- **"Now Playing" follows you across the app.** While a game is running on your appliance, a pulsing badge in the top bar shows the game name, system, and elapsed play time on every page. The home page replaces "Last Played" with a hero card for the active game, and the game's detail page picks up a "Now Playing" pill so you know you're looking at the one that's running. Tap the badge from anywhere to jump to that game's detail page.
- **One tap to the manual.** The Now Playing card has a "Manual" button that takes you directly to the manuals section of the running game — no scrolling, no hunting.
- **"Rescan Library" now matches what's on disk.** ROMs you delete manually no longer linger in your library. A rescan reconciles additions, updates, and removals in one pass.
- **Large NFS rescans are much faster.** When ROM files have not changed, Replay Control no longer re-reads large cartridge ROMs just to confirm the same game identity. Manual "Rebuild Library" is still available when you want a full verification pass.
- **Faster cold boot.** Library startup work is streamlined so systems begin appearing sooner, with artwork and metadata filling in as each system finishes instead of waiting for the whole library.
- **Rebuild is now safe to interrupt.** If storage drops, NFS hiccups, or power is lost during a rebuild/rescan, already-cached library data is preserved instead of disappearing or being written to the wrong storage.
- **Metadata coverage is much better.** More games now get release dates, descriptions, developers, genres, ratings, and player counts from LaunchBox and built-in catalogs, including harder cases such as arcade clones, alternate regional titles, and filename-only matches.
- **Full arcade sets are easier to browse.** Replay Control now keeps arcade entries from the source catalogs even when they are outside the usual coin-op game set, so full MAME-style libraries can show names and metadata for more files instead of falling back to raw filenames.
- **Local ROM auto-detection failures are visible.** If the filesystem watcher cannot start, the app now shows a banner explaining that new ROMs need a manual rescan or restart instead of silently missing changes.

### Added

- New "Now Playing" detector: a Linux-only background loop in `replay-control-app/src/api/now_playing.rs` polls every 4 seconds, walks `/proc/<pid>/maps` for a non-menu libretro core and `/proc/<pid>/mem` heap for the active ROM path, and broadcasts state via the new `state.now_playing_tx` channel. Two-poll debounce on `(pid, system, filename)` filters mid-launch noise, and a PID cache verified via `/proc/<pid>/comm` keeps the steady-state `/proc` walk to a single file read.
- `Resource<NowPlayingState>` provided at the App root with `Resource::new_blocking(get_initial_now_playing)` SSR seed so a hard refresh while a game is running paints the live state on the first frame (no hydration mismatch, no flash). After hydration `/sse/now-playing` writes new states straight into the same Resource via `Resource::set` — single source of truth.
- New `Clock` (1 s tick) + `use_now_playing()` + `use_live_elapsed_secs(started_at)` hooks in `replay-control-app/src/hooks/` so elapsed timers update smoothly between SSE events without bloating the wire payload.
- New shared `replay-control-core-server/src/replay_proc.rs` with `find_replay_pid`, `pid_is_replay`, `maps_have_active_game_core`, and a single `NON_GAME_CORES` exclusion list (`replay_libretro`, `avtest`).
- New `<NowPlayingIndicator>` component (top-bar pill) and `NowPlayingHeroCard` (home page).
- New `use_focus_scroll` hook (`ResizeObserver` on `<body>`, manual-scroll override) — the home-hero "Manual" button deep-links to `#manuals` and lands on the section even when cover-art images and lazy sections finish laying out after the initial scroll.
- New `MANUALS_FRAGMENT` const in `pages::game_detail` consumed by the home hero card link.
- New `docs/features/now-playing.md` covering the user surfaces, the `/proc` detection algorithm, robustness defenses, and perf numbers.
- `game_library.normalized_title` and `normalized_title_alt` columns populated at scan time (arcade clones store the parent's normalized title in `_alt`); enrichment matching is now a hashmap probe against stored keys instead of a per-ROM `normalize_title()` call. `launchbox_alternate.normalized_alternate` mirrors this on the LaunchBox side. Schema bumped to v4 with an `ALTER TABLE` migration that preserves existing libraries.
- New per-storage `library_meta` k/v table (first inhabitant: `title_norm_version`) and a host-side `external_meta.title_norm_version` stamp. `replay_control_core::title_utils::TITLE_NORM_VERSION` (currently `1`) is bumped any time `normalize_title_for_metadata` changes its output. On boot, mismatch on either side rebuilds the stored normalized columns silently — future matcher improvements reach deployed appliances on the next reboot without user action.
- New `match_for_rom` chain in `replay-control-core-server/src/library/enrichment.rs`: primary `normalized_title` → arcade-clone parent's `normalized_title` → `launchbox_alternate.normalized_alternate` → No-Intro `hash_matched_name` canonical filename (probed against both primary and alt-name maps). Stops at the first hit; strength descending.
- New `game_library.hash_size_bytes` migration. Existing CRC cache rows with a matching mtime are reused and self-heal by writing the observed file size on the next scan, avoiding a one-time post-upgrade rehash storm on large NFS libraries.
- New storage-generation cancellation token for long scan/rebuild work. Storage swaps bump the generation, stale scans return a typed `StorageChanged` cancellation, and write boundaries re-check the token before mutating `library.db`.

### Changed

- "Rescan Library" no longer just adds. Each visible system is reconciled to current disk state via a per-system strict scan that errors on permission / I/O failures, so a flaky NFS mount can't silently truncate the cached ROM list. Missing top-level system dirs become empty rows; recursive read failures preserve the previous cache.
- `phase_auto_import` is now the single entry point that re-parses the cached LaunchBox XML on boot. It checks both `launchbox_xml_crc32` and `title_norm_version` in one read; either mismatch triggers a re-parse, and `refresh_launchbox` writes both stamps inside the same transaction. The previously-separate `reconcile_external_normalized_titles` path is gone, removing a known race where the secondary writer would `pool unavailable` while the work was actually still committing on the deadpool thread.
- LaunchBox-sourced release dates now flow through `game_release_date` via `upsert_release_dates(source="launchbox")` *before* `resolve_release_date_for_library` runs. The resolver rebuilds `game_library.release_date` from the precision-aware table, so the previous wipe-on-resolve behavior (which zeroed the column for any system whose catalog had no `console_release_dates` rows) is gone. Day-precision LB dates upgrade year-precision catalog rows; year-precision LB dates fill systems with no catalog data at all.
- `launch.rs::check_game_loaded` (used by the post-launch health-check) now consumes the shared `replay_proc` helpers, picking up the `avtest` exclusion that was previously only applied by the `pgrep`-based check.
- `system_display_name` hoisted from `core::game_ref` to `core::platform::systems` so non-`GameRef` callers can use it.
- Generic `install_sse_listener::<T>(url, on_payload)` helper extracted from the duplicated Activity / NowPlaying / Config listener boilerplate in `lib.rs`.
- The metadata-page "Rescan" button copy and supporting docs (`docs/features/{game-library,getting-started,storage}.md`) updated to reflect reconcile semantics.
- **Local storage** (SD/USB/NVMe) now removes cached library rows immediately when an entire system folder is deleted, instead of waiting for an explicit rescan. NFS storage continues to preserve cached state if a folder appears missing (transient mount blip protection).
- Rebuild progress text changed from "Scanning Super Nintendo (3/41)..." → "Enriching ..." sequence to "Rebuilding Super Nintendo (3/41)..." with an "(enriching)" suffix added mid-iteration. The fleet-wide `RebuildPhase::Enriching` enum variant is gone — the per-system label carries the per-system phase signal.
- `populate_all_systems` collapsed to a single per-system pass: iterate `visible_systems()`, strict-scan + inline-enrich per system, drop the post-loop second pass. `spawn_rebuild_enrichment` and `spawn_cache_enrichment` updated to avoid double-enrich. `PopulateProgress` collapsed from three variants to two (`Startup`, `Rebuild`); rescan vs rebuild distinction lives on `RebuildProgress::is_rescan`.
- Strict reconcile rule for `scan_and_cache_system`: a successful filesystem read replaces L2 for that system; a failed read returns `Err` and preserves L2. Missing top-level system dir splits by `storage.kind.is_local()` — local treats as user deletion (reconcile-to-empty), NFS treats as ambiguous (preserve). Rebuild and watcher paths no longer pre-clear L2 — the previous "rebuild during NFS hiccup wipes your library" vector is closed.
- CRC identification now validates cached hashes with `mtime + size`. Normal rescans also allow conservative same-size reuse when only mtime drifts, which avoids streaming unchanged N64/GBA/SNES-era ROMs over NFS. Manual "Rebuild Library" sets `force_rehash=true` and recomputes CRCs for every hash-eligible ROM.
- Cached hashes are loaded once per system by the background orchestrator instead of from the writer-only scan path. `LibraryWritePool::as_reader()` was removed, keeping read and write pool boundaries honest.
- Hybrid cartridge/CD systems are handled per file: Sega 32X cartridge ROMs remain hash-eligible, while Sega CD 32X disc images are skipped even when they use `.bin` tracks.
- Arcade catalog builds now retain source entries for categories such as gambling, slot machine, computer, handheld, and electromechanical instead of filtering them out. Those rows are still sourced from MAME/FBNeo/Flycast metadata and normalize into the existing genre taxonomy where possible.
- `docs/features/benchmarks.md` now includes measured beta.9 NFS library-maintenance timings on the Pi/NFS development library (95,495 ROMs across 41 systems): fresh startup verification ~4.5 s, manual rescan 194.1 s, manual rebuild 636.0 s.
- Removed `scan_systems` and the silent walker family (`list_roms`, `walk_raw_roms_blocking`, `collect_raw_roms_recursive`), `count_roms_recursive`/`count_roms_inner`, `m3u_has_target_on_disk`, `ScanError::AllSystemsMissing`. The strict walker variants drop their `_strict` suffix and become the only walker. `find_duplicates` and the `library_report` binary migrated to the strict API. `StorageProbe::HasRoms` renamed to `HasVisibleEntries` — the new probe is a depth-1 dirent check (no recursion). `StorageProbe`'s strict-walker assumption was validated on a Pi NFS rig: drop-caches + immediate `read_dir` returns the correct entries synchronously, never spurious `Ok(empty)`.

### Fixed

- The home hero "Manual" deep-link now lands the user on the manuals section even on slow connections — `use_focus_scroll`'s `ResizeObserver` re-anchors `scrollIntoView` until either layout settles or the user manually scrolls.
- `use_focus_scroll`'s one-shot `requestAnimationFrame` and `setTimeout` callbacks now use `Closure::once_into_js` (auto-dropped after firing) instead of `forget()`. The `ResizeObserver` callback is stored in the cleanup teardown so it drops with the observer instead of leaking per Effect run.
- Per-system game descriptions are now cleared when a rescan empties a system. Previously the rows leaked past the ROM rows so a removed game's description could surface on a re-added ROM with a different filename.
- Closed the LaunchBox release-date wipe: 6,428 primary-LB matches that were silently dropping their date now persist. The alt-name + hash-name match chain additionally fills ~35,000 latent cells across genre / players / rating / developer / description / publisher where the alternate-name path had matched but field propagation was missing.
- Dropped a hydrate-mode warning on the home page: `now_playing_detail` was a derived `Resource::new(source = move || now_playing.get(), …)` whose source closure ran in a Memo owner that doesn't inherit `SuspenseContext`. The detail fetch now happens inline inside the existing `Suspend::new(async {})` block, where every `now_playing.get()` read sits cleanly in scope. App-root `<Suspense>` wraps around the top-bar header and the `/` and `/games/:system/:filename` route views provide context for the lazy `class:` and `<Show when=>` closures that read `now_playing` from a `RenderEffect`.
- Storage swaps during a long rebuild/rescan no longer risk stale writes into the newly-active storage DB. The cancelled activity now reports a neutral cancelled terminal state, and the new-storage startup verification retries claiming the activity slot while the old scan unwinds.
- ROM watcher tasks restart after storage swaps, so file changes are watched under the new active `roms/` root instead of the previous storage path.
- Region preference changes no longer clear the library cache. They now update the region-dependent release-date mirrors in place, avoiding a race where changing preferences during rebuild/import could wipe systems that had already finished.
- Local ROM watcher startup failures now broadcast a status event and render a sticky banner. NFS or intentionally skipped watcher states remain informational and do not show the warning.

---

## [0.4.0-beta.8](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.8) - 2026-05-04

### Highlights

- This beta focuses on diagnostics and large-library stability: startup logs now identify the exact build, `/settings/logs` can switch Replay Control between info/debug for the next reboot and copy logs to the clipboard, and the external metadata reader path is less likely to starve UI requests during very large scans.

### Changed

- External-metadata read pool increased from 1 to 2 connections. Large background enrichment and thumbnail-planning reads no longer monopolize the only reader, so short UI/server-function reads have a second slot during big-library scans.
- Thumbnail planning now releases its `external_metadata.db` read connection after loading manifest rows. Fuzzy matching and filesystem checks run off-pool, reducing reader starvation during thumbnail updates.
- Startup logs now include the same Replay Control version and git hash shown on the Settings page, making remote log captures easier to tie to an exact build.
- WAL databases now use SQLite's automatic checkpointing instead of disabling `wal_autocheckpoint` and forcing broad manual checkpoints after heavy write phases.

### Added

- `/settings/logs` now includes a Replay Control log-level selector. It writes the selected `RUST_LOG` value to `/etc/default/replay-control` and tells the user that changes apply after reboot; it does not restart or reboot the system.
- `/settings/logs` now has a copy-to-clipboard action next to Refresh, with a fallback path for browser contexts where `navigator.clipboard` is unavailable.

### Fixed

- Read-pool connections no longer attempt `PRAGMA optimize` while marked `query_only`, removing repeated `attempt to write a readonly database` recycle warnings.
- The logs source dropdown now binds its displayed value explicitly, fixing a blank collapsed select where the options only appeared after opening the dropdown.
- Large scans no longer force a post-scan `library.db` checkpoint through the generic 15-second write path, avoiding misleading write-acquire timeout bursts after successful scans.

---

## [0.4.0-beta.7](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.7) - 2026-05-04

### Highlights

- **External-metadata pipeline redesign.** LaunchBox text and libretro thumbnail manifests move out of per-storage `library.db` into a host-global `external_metadata.db` at `/var/lib/replay-control/external_metadata.db`. Newly added ROMs (after a LaunchBox refresh) automatically pick up metadata on the next enrichment pass — fixes the long-standing one-shot-import bug. Storage swaps no longer re-parse the 460 MB XML; only the binary keeps a stamp of the last-parsed file's CRC32. Image files stay per-storage at `<storage>/.replay-control/media/`, only the manifest of available filenames moves host-global.
- **`library.db` schema bumped 1 → 3** in two migration steps. v2 drops `game_metadata`, `thumbnail_index`, `data_sources` (relocated to `external_metadata.db`). v3 adds `game_description` (description + publisher denormalized so the game-detail page stays on a single pool). A downgrade guard refuses to open a DB stamped with a newer version than the binary.
- **One-button metadata refresh.** The legacy "Import LaunchBox metadata" UI is replaced by a single host-global "Refresh metadata" button that downloads, parses, and re-enriches every system in one flow with live SSE progress (Downloading → Parsing → Enriching → Complete). New `Activity::RefreshExternalMetadata` SSE variant — clients should add a render branch.
- **Game-detail page request path is now strictly single-pool.** Description, publisher, ratings, genre, and image stats all read from `library.db`; the host-global `external_metadata.db` is touched only at enrichment time.
- **Per-system enrichment writes collapse to one transaction.** Six separate `db.write` calls per system (developer / cooperative / year / release-date resolver / game_description / box-art-genre-rating) are now bundled into a single library-pool write — saves ~1.5 s of per-commit fsync across a 30-system re-enrichment on Pi.

### Added

- New host-global `external_metadata.db` (LaunchBox `launchbox_game` / `launchbox_alternate`, libretro `thumbnail_manifest` / `data_source`, `external_meta` key-value).
- New per-storage `game_description (system, rom_filename, description, publisher)` table in `library.db`. Truncate-and-repopulate per system on every enrichment pass.
- `Activity::RefreshExternalMetadata { progress }` SSE variant + `RefreshMetadataPhase` (Checking → Downloading → Parsing → Enriching → Complete/Failed/**UpToDate**) + `RefreshMetadataProgress` (source_entries, downloaded_bytes, elapsed_secs, error). Mirrored on both SSR-side `api::activity` and WASM-side `types`.
- `BackgroundManager::spawn_external_metadata_refresh` and `spawn_external_metadata_download_and_refresh` for UI-triggered refreshes (regenerate, download).
- `library_db::resolve_launchbox_xml(cache_dir, storage_rc_dir)` — single helper that picks the LaunchBox XML across the host-global cache and per-storage legacy locations (boot-time hash check + UI download both use it).
- `replay_control_core::title_utils::normalize_title_for_metadata` — single canonical normalizer used by both the import-time index and the per-row read-time lookup, so the two sides can never drift.
- Setup checklist's "metadata imported?" now reads `external_metadata.launchbox_game`'s row count.
- First-boot data seeding (Phase 0.5): on a fresh install, the startup pipeline silently downloads the LaunchBox XML and libretro thumbnail manifest before the first ROM scan so initial enrichment has full data. Network failures are warn-logged and the pipeline continues — offline-ready behaviour is preserved. Subsequent boots skip the phase entirely.
- `launchbox::fetch_upstream_head()` — single HEAD request to the LaunchBox ZIP URL that returns both ETag and Content-Length, eliminating the two-subprocess pattern used by the former download flow.

### Changed

- Boot pipeline Phase 1 (`phase_auto_import`) is now a content-derived hash check + refresh against `external_metadata.db` — replaces the legacy "DB-empty" gate that broke when ROMs were added after a one-shot import. Hash + stamp-read run in parallel via `tokio::join!`. After refresh, every active system is re-enriched so launchbox data flows through `game_library` + `game_description`.
- Enrichment now reads launchbox via a single batched per-system query (`external_metadata::system_launchbox_rows`) instead of seven separate one-field queries. Per-ROM lookup uses normalized-title candidates (handles arcade clones via the parent's display name).
- Image index construction (`build_image_index`) drops its DB-connection arg — it's now a pure filesystem walk plus the pre-loaded libretro repo data, called from `tokio::task::spawn_blocking`.
- Game-detail page lookup of description + publisher reads from per-storage `game_description` (single library-pool acquire) instead of cross-pool acquiring `external_metadata.db`.
- `LibraryDb::all_ratings`, `image_stats`, `rom_genre` are now sourced from `game_library` (already populated by enrichment) instead of the dead per-storage `game_metadata` table.
- Download-progress callback throttled from per-64-KB-chunk to every-1-MB so the activity SSE channel doesn't churn 3 200 lock+broadcast cycles per 200 MB download.
- Parse-progress now updates the activity stream every 5 000 entries so the UI banner shows a live counter during the 30–90 s LaunchBox parse.
- "Refresh metadata" now performs an HTTP ETag check before downloading — the stored `launchbox_upstream_etag` key in `external_meta` is compared against the server's current ETag via a single HEAD request. On match, the flow short-circuits to `RefreshMetadataPhase::UpToDate` and shows an "Already up to date" result for 5 seconds, skipping the 100+ MB download entirely. Clearing metadata also clears the stored ETag so a post-clear refresh always re-fetches.

### Removed

- Per-storage `game_metadata`, `thumbnail_index`, `data_sources` tables (relocated to `external_metadata.db`).
- Legacy `LibraryDb::bulk_upsert` / `lookup` / `system_metadata_*` / `clear` / `is_empty` / `delete_orphaned_metadata` / `bulk_update_image_paths` / `system_box_art_paths` / `entries_per_system` / `stats` and the `GameMetadata` / `MetadataStats` / `DataSourceInfo` / `DataSourceStats` / `ThumbnailIndexEntry` / `ImagePathUpdate` types.
- Legacy `library/imports/launchbox.rs` import functions (`import_launchbox`, `run_bulk_import`, `build_rom_index`, `build_index_entries`, `import_launchbox_aliases`) and the `library/matching/metadata.rs` auto-match module — replaced by `library/external_metadata_refresh.rs::refresh_launchbox`.
- Legacy `api::ImportPipeline` and the `import_launchbox_metadata` server fn — replaced by `BackgroundManager::spawn_external_metadata_refresh` + `spawn_external_metadata_download_and_refresh`.
- Legacy `cleanup_legacy_metadata_db` (pre-0.5 `metadata.db` cleanup); the upgrade path is now far past it.
- `update_image_paths_from_disk` and the `ImagePathUpdate` flow (legacy thumbnail-download path that wrote `box_art_path` to `game_metadata`); the new flow writes `box_art_url` directly to `game_library`.

### Fixed

- Recents list arcade-display-name resolution is now one catalog round-trip per system instead of one per ROM (N→1), matching the batch approach already used by favorites.
- "Update Thumbnails" no longer re-fetches the libretro manifest from GitHub (~70 API calls) when clicked a second time within 5 minutes — the last-fetched timestamp is stored in `external_meta` and used as a 5-minute TTL gate.
- "Refresh metadata" clicked when no upstream change occurred now shows "Already up to date" in the result strip for 5 seconds instead of producing no visible feedback. The ETag check runs in the `Checking` phase so the banner is visible before the result is known.
- ROMs added to a system after a one-shot LaunchBox import are now enriched on the next pass (was: silently skipped forever).
- Two concurrent boots (boot pipeline + storage-watcher restart) no longer race the LaunchBox refresh — the activity slot is claimed before the hash check, so the second caller cleanly bails.
- Activity SSE no longer flickers `Idle` between the download and parse phases of a one-button refresh — the guard is threaded from the download path into `phase_auto_import_inner` via an explicit parameter.
- Neo Geo (`snk_ng`) re-categorized from `Console` → `Arcade` so MAME-shortname ROMs (`mslug.zip`, `kof98.zip`) route through `arcade_db` instead of failing to match LaunchBox.

### Migration / Upgrade Notes

- **Downgrade is not supported.** Once `library.db` is stamped with v3, an older binary refuses to open it (the downgrade-guard check in `LibraryDb::run_migrations` raises an error). Roll forward only.
- First boot after upgrade re-parses the LaunchBox XML (~5–8 minutes on Pi at the typical ~150 K-game XML size) because the new `external_metadata.db` starts empty. Subsequent boots are no-op when the XML hash matches the stamp.
- Existing per-storage `<storage>/.replay-control/launchbox-metadata.xml` continues to work — the refresh path checks the host-global cache first, then falls back to the per-storage location.

---

## [0.4.0-beta.6](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.6) - 2026-05-03

### Highlights

- The "library shows no metadata after upgrade" silent-failure mode is now caught and surfaced. Beta.4-to-beta.5 upgrades that replaced the binary without refreshing `catalog.sqlite` (auto-update from a release whose updater predated catalog-swap) are detected at startup; arcade lookups short-circuit cleanly instead of spamming per-row SQL errors, and the new `<AssetHealthBanner>` tells the user to reinstall. Generic enough that future shipped-asset incompatibilities (themes, fonts, …) plug into the same surface.
- Production fd exhaustion under heavy thumbnail fan-out is fixed structurally. A new `ThumbnailDownloadOrchestrator` replaces the previous "every cache-miss spawns a `tokio::spawn`" pattern with a bounded concurrency cap, shared dedup, and visible-vs-bulk priority. Beta.5 telemetry showed 1 012 / 1 024 fds open mid-rescan with 993 sockets — that class of failure can no longer occur.
- NFS slow-mount on cold boot no longer kills startup. The 15 s `STORAGE_READY_TIMEOUT` that put beta.5 NFS users in a `Restart=on-failure` loop is gone; the not-ready case routes through the existing `/waiting` page until the mount surfaces, and the background re-detection loop activates storage as soon as it does.
- Cold-cache rebuilds across the home and metadata pages no longer trip the 15 s `INTERACT_TIMEOUT` tripwire on 100 k+ ROM libraries. `metadata_page_snapshot` split its 8-query closure (then parallelized via `tokio::join!`); `bulk_insert_aliases` chunks into 5 k-row transactions; `get_recommendations` moved to the same `SsrSnapshot<T>` pattern as the metadata page (event-driven invalidation, single-flight rebuild, stale-on-`None`). Per-ROM warmup rate improved ~7×.
- Game-detail page now has a unified lightbox carousel covering box art, title screen, in-game screenshot, and user captures — tap any image and swipe through them all.
- `/media/*` and `/rom-docs/*` get HTTP `ETag` + 304 revalidation on top of the existing 1-day `Cache-Control`. Box art / thumbnails / marquees that dominate game-grid traffic now revalidate body-less when the browser's max-age expires.

### Added

- HTTP `ETag` + 304 revalidation on `/media/*` and `/rom-docs/*`. Strong tag derived from `mtime + size`; once a browser's 1-day `max-age` expires, the next reload sends `If-None-Match` and the server replies with a body-less 304 if nothing changed instead of re-shipping the bytes. Box art / thumbnails / marquees see the biggest win — they dominate game-grid traffic. Hot path adds one `tokio::fs::metadata` call on cache-miss; on warm page-cache it's noise. Cache-Control max-age stays at 1 day.
- Game-detail lightbox now covers box art, title screen, in-game screenshot, and user captures as a single carousel — tap any image and swipe through them all. Per-image rendering hint (`LightboxImage { url, pixelated }`) keeps nearest-neighbour upscaling on pixel-art screenshots while letting box-art covers scale smoothly. Combined image list is reactive, so picking a new cover via the picker updates the lightbox in place.
- `ThumbnailDownloadOrchestrator` (`replay-control-app/src/api/thumbnail_orchestrator.rs`) — single coordinator for all thumbnail-download work with bounded concurrency (`Semaphore::new(10)`), shared dedup across pipelines, priority via two channels + `select! biased` (visible preempts bulk), per-job completion delivery, and `AtomicUsize` in-flight + `AtomicU64` lifetime counters. Wired through the on-demand box-art enrichment path in `library/enrichment.rs::queue_on_demand_download` to fix the production fd-leak: previously every cache-miss did an unbounded `tokio::spawn`, so a fresh-system rescan with thousands of missing thumbnails opened thousands of HTTP sockets simultaneously and burned through the 1024 fd soft limit (993/1012 fds were sockets in beta.5 telemetry). Bulk pre-fetch path keeps its existing local Semaphore for now — wiring it through the orchestrator is a follow-up that requires migrating its `Activity`-state progress callback to the completion-channel model.
- "Rescan Library" button on the metadata page — additive rescan of all systems without touching previously-imported metadata. Surfaces under the same activity-gating as the existing import; the button disables itself when another metadata operation is running so two concurrent rescans can't race the L2 write path.

### Changed

- Tapping the box art on the game-detail page now opens the lightbox instead of the variant picker. The "Change cover ›" link below the cover is now the only entry point to the picker. Cleaner separation of concerns: tap = view, link = swap.
- `metadata_page_snapshot::compute` no longer bundles all 8 stats queries into a single `pool.read` closure. The closure was the right shape on small libraries but became a problem at scale — on the 141k-ROM beta.5 reporter it ran 80–170 s, well past the 15 s `INTERACT_TIMEOUT` tripwire, and held a read-pool slot the whole time so concurrent SSR requests starved behind it. Now 8 small `pool.read` calls fanned out via `tokio::join!`; the pool's 3 read slots overlap them instead of running back-to-back, no individual closure exceeds the cap, and SSR readers can slot in between.
- `bulk_insert_aliases` chunks into 5 000-row transactions instead of one monolithic transaction. The user-supplied beta.5 log showed `library_db: write exceeded 15s` mid-LaunchBox-import on a ~30 k-alias batch; chunking keeps each transaction well under the cap. `INSERT OR REPLACE` is row-idempotent so cross-batch atomicity is not required (a power loss mid-import re-inserts cleanly on the next run).
- `get_recommendations` migrated from a 5-minute `TtlSlot<RecommendationData>` to the existing `SsrSnapshot<T>` pattern (already used by `metadata_page_snapshot`). Strictly better caching: event-driven invalidation via the same write-completion sites that already invalidate the metadata snapshot, single-flight rebuild on miss, stale-on-`None` so the home page keeps rendering during long writes. Cold-case behaviour: see follow-up item under Internal — the planned cold-instant-return is captured as a pending task in the beta.5 NFS investigation doc. New `AppState::invalidate_user_caches()` helper consolidates the parallel `response_cache.invalidate_all()` + `cache.invalidate_recommendations()` calls so they stay in lockstep across ~22 write sites.
- Enrichment setup (`enrich_system_cache`) hoists `visible_filenames` once instead of querying it twice (`auto_match_metadata` was independently re-fetching the same rows — a per-system N+1). `build_image_index` + `auto_match_metadata` + `ArcadeInfoLookup::build` then run in `tokio::join!` so they overlap on the pool's read slots; bails early when the system has no visible filenames.
- LaunchBox import end-of-run summary now logs the *real* metadata-row count (`COUNT(*) FROM game_metadata`-equivalent) — typically 2–3× the matched-ROM count due to regional variants. Previously the line read "0 inserted" because the parser-side counter is always 0 in the bulk-import path (the writer task publishes the real count via an atomic, patched into stats after both tasks join). New format: `LaunchBox import: N source entries, M matched ROMs, K metadata rows inserted, S skipped`. The misleading parser-local log demoted to `debug!` and re-tagged "LaunchBox parse:".

### Fixed

- Catalog schema mismatch when a beta.4 → beta.5 upgrade replaced the binary without refreshing the bundled catalog. `init_catalog` now compares the `arcade_games` column set against `ARCADE_COL_NAMES` at startup (reuses the library's `table_columns_diverge` primitive). On divergence: log one loud journal `ERROR` directing the user to reinstall, set the `CATALOG_SCHEMA_OUTDATED` flag, and short-circuit `with_catalog` so subsequent arcade lookups return `None` instead of spamming `no such column: source` per row. Surfaced in the SPA via the new `<AssetHealthBanner>` (`api::AssetHealthIssue` + `ConfigEvent::AssetHealthChanged` + `replay-control-core::asset_health`) so the user sees the banner immediately on page load.
- Drop the 15 s `STORAGE_READY_TIMEOUT` from `wait_until_mount_point` (renamed to `is_ready` — now a one-shot bool check). `prepare_storage_dbs` no longer fails startup on slow NFS first-mount; instead, the detect site routes the not-ready case into the existing no-storage path (which already redirects every request to `/waiting`). The background re-detection loop picks up the mount when it appears. Beta.5 NFS users were hitting "Storage not ready: did not become a mount point within 15s" → service exit → `Restart=on-failure` cycle; the new model keeps the service up indefinitely with a clear UI signal until the mount surfaces. `refresh_storage` gets the same gate so a transient mount-not-ready blip doesn't tear down a working storage state.
- Two stale slugs in the libretro-thumbnails repo mapping: `Atari - 7800 ProSystem` was renamed upstream to `Atari - 7800` (the old slug 404s); `Philips - CDi` was renamed to `Philips - CD-i` (added a hyphen between CD and i). Both 404s appeared in beta.5 telemetry. Stopgap fixes in the hardcoded `mod.rs` table; the real fix is catalog-build-time slug resolution from the live GitHub org listing — separate design pass.
- `build.rs` now emits `cargo:rerun-if-changed=../.git/HEAD` and `../.git/index` so the embedded `GIT_HASH` doesn't go stale on incremental builds. `/api/version` was reporting the *previous* commit's hash after a deploy of new code (the binary itself was always correct; only the displayed string lied).
- NeoGeo AES and MVS systems now route through the arcade metadata path. They were previously treated as console systems, so MAME / FBNeo curated names didn't apply and game listings showed raw ROM filenames.
- `ThumbnailDownloadOrchestrator::submit_visible` / `submit_bulk` no longer leak a dedup-set entry when the calling future is cancelled mid-await. New RAII `ClaimGuard` rolls back the claim on drop unless `disarm()` is called after a successful send. Without the guard, a cancelled submit between `try_claim` and `send().await` would leave the key in the pending set forever, silently dedup-skipping every subsequent submit for that thumbnail.

### Internal

- Per-connection WAL-fallback log line in `sqlite.rs::open_connection` demoted from `info!` to `debug!`. The "filesystem does not support WAL, using DELETE journal" message was firing on every connection open against an exFAT / NFS DB — 4× per startup with no actionable content. The two real fallback paths (`open_wal` failed, `open_nolock` failed after FS reportedly unsupported WAL) keep `info!` since those indicate something unusual.
- `db_pool::dispatch` now logs every error path explicitly (`Corrupt`, `Busy`, `Closed`, `RwLock-poisoned`, deadpool acquire failure). Five of seven `DbError` variants were silent — investigations of "`pool.read` returned `None`" had to guess at the cause. Now: `debug!` for transient/expected states (closed during shutdown, gate during DELETE-mode writes, corrupt while recovery runs), `warn!` for connection-acquire failures, `error!` for poisoned RwLock and the existing 15 s timeout.
- `dev.sh` seeds `RUST_LOG=info,replay_control_app=debug,replay_control_core=debug,replay_control_core_server=debug` for dev bootstraps so the new diagnostic logs surface in dev-Pi logs immediately. `install.sh` keeps the `info`-only default for shipped installs.
- Code-review pass on the beta.6 cycle: extracted `LibraryDb::update_box_art_url` helper (deduped 3 raw SQL sites in `boxart.rs` and `enrichment.rs`); collapsed `submit_visible` / `submit_bulk` enqueue logic into a shared helper; collapsed `Outcome::DownloadFailed | SaveFailed` arms in the on-demand on-complete hook; dropped a dead `_count` parameter from `get_recommendations`; trimmed change-history narration from several files. No behaviour change.
- E2E suite fixes for CI failures introduced by the beta.5 path move + an `ls` / `test -f` bug in the Pi storage fixture. `tests/integration/run.sh` and the affected Playwright cases now exercise the post-storage-id paths correctly.
- `cargo clippy` cleanup pass on `asset_health_banner` and `recommendations` server-fn signatures.

---

## [0.4.0-beta.5](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.5) - 2026-04-30

### Highlights

- The `/settings/metadata` page no longer hangs on rapid force-refresh and stays interactive throughout long-running imports and thumbnail updates. The fix is structural — a single in-memory page snapshot replaces six fan-out server fns, with single-flight rebuild and stale-on-`None` fallback.
- Page transitions feel snappier across the board on Pi 4 / USB+exFAT. The response-level cache TTL is 5 minutes (was 10 s), so the recommendations / favorites carousels stay warm across navigation pauses instead of paying a 100–300 ms recompute on the next click.
- A stale-NFS race that occasionally wiped the cached system metadata (and made the library look empty until a manual recovery) is fixed at four layers — readiness check, scan-error signalling, SQL-level zero-overwrite guard, application-level warning.
- Thumbnail update finally emits structured logs (`Manifest import: starting / complete`, `Thumbnail update done: …`) instead of being silent. GitHub API rate-limit responses are detected and surfaced once with a "configure GitHub API key" hint.
- Auto-update now downloads and swaps the bundled `catalog.sqlite` atomically alongside the binary and site assets, with a clean rollback path if the swap fails.
- Arcade ROM names and metadata now respect each system's upstream curation. `arcade_fbneo` shows FBNeo's `"Galaga '88"`; `arcade_mame` shows MAME's name as-is for the same ROM. Cross-source field merge fills gaps too — e.g. on `arcade_fbneo` an FBNeo row with no rotation tag falls back to MAME's `vertical`. Resolution order per system: see `arcade_source_priority` in `replay-control-core/src/platform/systems.rs`.
- The library database now lives centrally on the host SD card at `/var/lib/replay-control/storages/<storage-id>/library.db`. Each ROM storage gets a stable id derived from its filesystem UUID, so re-plugging a USB after a reboot keeps every cached row — no rescan, no rematch, no enrichment delay. User overrides and saved videos still travel with the storage on `<storage>/.replay-control/user_data.db`. One-shot migration on first attach for users coming from beta.4.
- A "library shows 0 games" regression caused by per-connection WAL recovery unlinking sidecars under live connections is fixed at four layers — recovery is now scoped to pool open, lifecycle ops drain before unlinking, the write gate is mode-aware so WAL pools never block readers, and `try_read`/`try_write` return typed errors so cascade gates can no longer mistake "pool busy" for "library is empty".

### Added

- `MetadataPageSnapshot` — in-memory single-flight cache of the `/settings/metadata` payload. Six per-stat server fns (`get_metadata_stats`, `get_system_coverage`, `get_thumbnail_data_source`, `get_image_stats`, `get_builtin_db_stats`, `get_library_summary`) collapse to one `get_metadata_page_snapshot`. The compute path runs DB queries in one `pool.read` closure (single pool acquisition, single cancellation-orphan slot if the SSR future is dropped); any non-DB work follows after the connection releases. Pre-warmed at boot in `run_pipeline`; invalidated at every existing write-completion site.
- Generic `SsrSnapshot<T>` helper in `replay-control-app/src/api/library/ssr_snapshot.rs`. Future SSR pages that want "compute once per write cycle, share across concurrent requests, fall back to stale on transient unavailability" can opt in with one field declaration and one accessor. Backed by `RwLock<Option<T>>` + double-check inside the write lock; stale-on-`None` rule preserves the previous value when the builder returns `None` (DB transiently unavailable). Drives `metadata_page_snapshot` directly.
- 15-second `tokio::time::timeout` cap on every `conn.interact()` closure in `db_pool.rs`. The closure can't be cancelled (it's a `spawn_blocking` task and Tokio's blocking-pool work isn't cancellable on `JoinHandle` drop), but the awaiting caller bails with `Err(DbError::Timeout)` instead of hanging, and the offending site is surfaced via a loud `tracing::error`. Defense-in-depth against any future code path that re-introduces a slow closure.
- `PoolMetrics` atomic counters on `DbPool`: `reads_started/completed/returned_none/timed_out`, `writes_started/completed/timed_out`, `gate_blocked_reads`. Snapshot is `Serialize`/`Deserialize`. Cheap (single atomic load to read), wires straight into a future `/debug/pool` HTTP endpoint when needed.
- `DbError` typed errors on `DbPool::try_read` / `try_write`: `Closed`, `Corrupt`, `Busy`, `Timeout`, `Sql`, `Acquire`, `Interact`, `Other`. Replaces the `Option<R>` "anything went wrong = None" idiom that caused the visible "library shows 0 games" regression — cascade gates that read a row to decide "is the library empty?" can now distinguish *pool unavailable* (skip, retry later) from *query ran and returned no rows* (genuine empty state). Legacy `read()`/`write()` adapters remain as `try_*().ok()` for sites where best-effort is genuinely correct.
- `DbPool::reset_to_empty()` and `replace_with_file(src)` — the supported "clear and rebuild" / "restore from backup" entry points. Drain in-flight `Object`s before unlinking; abort the operation (returning `false`) if drain times out, so a stuck closure can't hold an fd into a deleted inode while a new pool opens at the same path. Both are atomic in the order: drain → unlink sidecars → mutate → reopen.
- Storage id (`<kind>-<8 hex>`, e.g. `usb-9a3a700d`) — derived deterministically from the filesystem identifier (volume UUID for block devices, `server:/share` for NFS) via CRC32. Self-healing if the marker file is lost (regenerates the same id). Random fallback only when no FS identifier is obtainable (tmpfs, exotic mounts). Kind tag (`usb` / `sd` / `nvme` / `nfs`) lets a glance at `/var/lib/replay-control/storages/` tell what each entry corresponds to. New `replay-control-core-server/src/storage_id.rs` and `data_dir.rs` modules.
- `--data-dir` CLI flag on `replay-control-app` for parking library DBs somewhere other than `/var/lib/replay-control` (NVMe, alternate mount). Default unchanged on Pi.
- `LibraryDb::SCHEMA_VERSION` + `run_migrations` framework: numbered, additive migrations (`ADD COLUMN`, `CREATE INDEX`, `UPDATE … WHERE …`) that preserve user-populated tables across schema bumps. Sits alongside the existing column-set-diff drop path that's still used for the four rebuildable derived tables (`game_library`, `game_library_meta`, `game_metadata`, `game_release_date`); migrations are the future-facing path for any table whose content shouldn't be flushed.
- Property tests on `DbPool`: `concurrent_writes_visible_to_all_readers` (forces lazy connection creation, asserts every reader observes every commit — the test that would have caught the WAL-unlink regression), `reset_to_empty_blocks_until_drain`, `crash_recovery_simulation`, `gate_blocked_read_returns_typed_error`, `closed_pool_try_read_returns_typed_error`, `corrupt_pool_try_read_returns_typed_error`, `wal_writes_do_not_block_concurrent_reads`. Plus `rebuild_corrupt_library_wipes_table_content` integration test that asserts a sentinel row inserted before `mark_corrupt` is gone after the rebuild — proves the lifecycle actually drains and unlinks rather than just flipping the flag.
- `ScanError` enum on `replay_control_core_server::roms::scan_systems`: `RomsDirUnreadable` and `AllSystemsMissing` distinguish "filesystem not yet ready" from "user genuinely has no ROMs". New `wait_for_storage_ready(roms_dir, timeout)` polls `read_dir` with backoff; called from `run_pipeline` before any scan. Defends against the NFS / autofs / USB-hot-plug race where the storage root resolves before subdirectories surface.
- `LibraryDb::save_system_meta` now refuses at SQL level to lower a non-zero `rom_count` to zero on UPDATE. Returns the post-write count so callers can detect and log when the guard fired. INSERTs into a fresh row are unaffected.
- Auto-update downloads, extracts, swaps, and rolls back `catalog.sqlite` alongside the binary and site assets. New `backup` / `swap` / `unbak` / `restore` shell helpers in `generate_update_script` keep the three swaps atomic with a single rollback path. Releases without a catalog asset (< v0.4.0-beta.3) skip the catalog step cleanly via an empty `CATALOG_SRC`.
- `install.sh --purge` wipes all on-Pi data (catalog, settings, env file, cached LaunchBox XML) for clean reinstalls.
- `install.sh --pi-pass` flag handles non-default RePlayOS SSH passwords from the curl-piped one-liner.
- `dev.sh` bootstraps the systemd unit + `/etc/default/replay-control` env file on the Pi when missing — mirrors what `install.sh` emits, runs as a no-op when the unit already exists.
- ReplayOS custom user skins (slots 11+) appear in the selector as a disabled `CUSTOM #N` entry instead of being invisible. Active-skin badge subscribes to the live `current_skin` signal so changes from the Pi reflect immediately.
- 22-case e2e test suite (`tests/e2e/test_page_health.py`) covering route-content health, navigation budgets, force-refresh resilience, server-fn registration. Plus a 3-case `tests/e2e/test_response_cache.py` that pins `RESPONSE_TTL` ≥ ~30 s. Integration suite (`tests/integration/run.sh`) fixed: the `/system/<x>` route assertions were checking the leptos 404 fallback (status-only check missed the regression) — now anchored on `/games/<x>` by content.

### Changed

- `arcade_games` catalog table restructured to row-per-source (PK `(rom_name, source)`). Replaces the previous one-row-per-rom schema where MAME current's last-write overrode every field, losing FBNeo's curated names like `"Galaga '88"`. The runtime `lookup_arcade_game(system, rom)` merges fields by per-system priority (`replay_control_core::systems::arcade_source_priority`); MAME's name wins on `arcade_mame`, FBNeo's on `arcade_fbneo`, with field-level fallback (e.g. FBNeo lacks rotation → falls through to MAME). `arcade_release_dates` gets per-source attribution as a side benefit. Catalog file 12.5 MB → ~14.8 MB; 27,272 rows for 15,439 distinct ROMs. PK index covers `WHERE rom_name = ?` via leading-prefix scan — no extra index needed.
- `WriteGate` is now pool-private (`pub(crate)`) and only auto-activates on DELETE-mode pools (exFAT/NFS user_data). On WAL pools (the library on the host SD) it is *never* set — SQLite's MVCC means writers don't conflict with readers, so gating is pure overhead and was actively harmful: the previous always-gate behavior caused the destructive `is_empty` cascades that wiped `box_art_url` after a thumbnail update. The gate is held only across a single `try_write` call; long write sequences (LaunchBox import, thumbnail manifest sync, populate_all_systems) drop the gate between batches, so SSR readers stay responsive throughout. `pool.read_through_gate` API and the public `WriteGate::activate(pool.write_gate_flag())` pattern are gone.
- `cache.invalidate(&db)` and `invalidate_system(system, &db)` return `Result<(), DbError>` instead of swallowing the L2 clear's failure. Destructive callers (`rebuild_game_library`) propagate so a no-op clear-then-rebuild can't silently write new rows over old ones (the same hazard pattern as the WAL-unlink regression). Cache-clearing afterthoughts on already-successful writes log at `debug` and continue.
- `import.rs::regenerate_metadata` and the three corruption-recovery server fns (`rebuild_corrupt_library`, `repair_corrupt_user_data`, `restore_user_data_backup`) migrated to the new lifecycle primitives. The previous `pool.close(); delete_db_files(); pool.reopen()` choreography is replaced by `pool.reset_to_empty()` / `pool.replace_with_file(backup)` — single atomic transitions, drain-aware, with `delete_db_files` now `pub(crate)` so future callers can't reintroduce the unlink-while-open hazard.
- `refresh_storage` and `AppState::new` now share `prepare_storage_dbs` (storage readiness, id assignment, library migration, path resolution) and `reopen_user_data_or_mark_corrupt` (header pre-flight). Adding a new pre-attach step now happens in one place — the previous parallel inline blocks drifted, which is how the storage-swap path missed the bad-header pre-flight that `AppState::new` had at startup.
- Library DB read-pool size: 3 connections (was effectively 1 across the pre-redesign + brief `read_bg` slot). WAL on ext4 SD lets concurrent reads actually parallelise; 3 covers SSR fan-out (recommendations + recents + favorites + system info) overlapping with one long enrichment / thumbnail-planning pass without queueing. User_data pool stays at 1 reader (DELETE-mode pool, the gate serialises against writers anyway).
- `cached_systems` now distinguishes three outcomes from `load_systems_from_db`: `Some(non-empty)` (cache hit), `Some(empty)` (DB reachable, no systems cached → fall through to filesystem scan), and `None` (DB transiently unavailable → return empty without caching, retry on next call). Avoids triggering an expensive multi-thousand-ROM scan on every transient pool unavailability.
- `cached_systems` no longer caches a poisoned result from a racy `scan_systems`. When the new `ScanError` fires, the L1 cache is left empty so the next caller retries once storage settles.
- Read-connection page cache bumped from 500 pages (~2 MB) to 1 000 pages (~4 MB). Recommendations / system-coverage / metadata-snapshot rebuild queries scan tens of thousands of rows; the bigger cache keeps hot indexes resident across calls. Write connection unchanged at 500 pages.
- `RESPONSE_TTL` in `api/response_cache.rs` raised from 10 s to 5 min. Every navigation pause longer than the old TTL paid a 100–300 ms recompute on Pi 4 / USB+exFAT, which surfaced as "stale browser load" on the next click. All write paths that *could* invalidate (favorites toggle, library invalidate, image clear, post-import cache invalidate) already call `response_cache.invalidate_all()`; the TTL is an upper bound when no write happens.
- `fetch_repo_tree` and `check_repo_freshness` route through a new `gh_api_get` helper that inspects status code + `X-RateLimit-*` headers. A 403 with `X-RateLimit-Remaining: 0` returns a structured `GhResponse::RateLimited { reset_unix, message }` instead of being mashed into an opaque error. `import_all_manifests` bails on the first rate-limit response (every subsequent request would hit the same wall) and emits a single user-actionable warning.
- Thumbnail update progress label format: `System · Boxarts 42% · 12 new, 87 cached` instead of `System: 7/15`. Banner uses `: ` separator instead of parentheses.
- Setup checklist surfaces an immediate "pending" state on click; the flag clears once SSE confirms the matching activity has started, avoiding the "did my click register?" flash.
- LaunchBox import is now pipelined: the sync XML parser sends parsed records over a channel to an async writer that drains them onto `pool.write` in batches. `replay-control-core-server` no longer needs `Handle::block_on` to bridge the two halves. ~40% faster on WAL-mode storage.
- `install.sh` no longer downloads the 489 MB LaunchBox metadata XML at install time. Fetch on demand from Settings → Download metadata when you need it; the catalog ships embedded with the binary.

### Fixed

- The user-reported `/settings/metadata` "second force-refresh hangs" pattern. Root cause was `deadpool-sync::SyncWrapper::interact()` running closures on a `spawn_blocking` task that doesn't cancel when the awaiting future drops — a force-refresh left an orphan closure holding the `SyncWrapper`'s inner mutex, blocking every subsequent `interact()` until it finished. Six per-page server fns multiplied the orphan count. Fixed structurally by collapsing to one acquisition per page and adding the 15 s wall-clock cap on `interact()`.
- The user-reported NFS startup race that wiped `game_library_meta` (every system reset to `rom_count = 0` after a reboot when NFS subdirectories hadn't materialised yet). Repro: `/media/nfs/roms` resolves but per-system folders are not yet listable; every `system_dir.exists()` returns false; `scan_systems` returned 41 zero-count summaries; `save_systems_to_db` UPSERTed all zeros over the previous boot's correct counts. Defends at four layers: `wait_for_storage_ready` at startup, `ScanError` signalling from `scan_systems`, SQL-level zero-overwrite guard in `save_system_meta`, application-level warning when the SQL guard fires.
- Thumbnail update used to be silent in `journalctl` — no INFO logs at all, only per-system warnings. A failed update (commonly: GitHub API rate limit on the unauthenticated 60 req/h cap, with the pipeline making ~70 calls per full run) left no diagnostic trace. Now logs entry / per-phase / completion lines and surfaces rate-limit failures with a "configure GitHub API key" hint.
- `manifest_import` no longer holds the `WriteGate` across the multi-minute GitHub HTTP loop. The gate is acquired per-batch inside `pool.write`, so SSR `pool.read()` calls succeed in the gaps between batches.
- `/system/<x>` style routes used by an earlier integration test never actually existed — they fell through to the leptos `Page not found` fallback while still returning HTTP 200. The integration suite has been corrected to assert `/games/<x>` and to anchor real-route detection by content (`Hide Hacks`, `All Genres`) instead of status alone.
- Settings page surfaces update-channel save errors instead of swallowing them.
- "Library shows 0 games" regression seen on a long-running Pi after rebuild or thumbnail-update operations. Root cause was `sqlite::recover_stale_wal` running unconditionally inside per-connection `open_connection`: a second concurrent connection in the same process triggered recovery, which checkpointed + switched journal mode to DELETE + unlinked `-wal`/`-shm`. The first connection kept its file descriptors but those inodes were now orphaned, so its reads saw only the pre-WAL state — i.e. an empty `game_library` if recent writes hadn't been checkpointed yet. Matching `/proc/<pid>/fd/` showed three live readers against the same main inode but only one with an intact WAL fd. Fixed by scoping recovery to a single one-shot call inside `DbPool::new` / `DbPool::reopen` (renamed `recover_after_unclean_shutdown`), and by making `delete_db_files` `pub(crate)` so the only public path to the WAL files is the drain-first lifecycle.
- Destructive `is_empty` cascade in `spawn_cache_enrichment` / `spawn_rebuild_enrichment` / `phase_cache_verification`. The previous `library_pool.read(...).await.unwrap_or(true)` pattern conflated "pool busy" with "library is empty", silently triggering full populate-from-filesystem (which DELETE+INSERTs `game_library` with no `box_art_url`) every time a cache-clear write happened to be in flight. Migrated to `try_read` + match: pool unavailability is "skip, retry later", never "library is empty".
- Schema rebuild on column-set diff for the four rebuildable derived tables (`game_library`, `game_library_meta`, `game_metadata`, `game_release_date`) was momentarily replaced with a `WARN` log during refactor, which would have left users with a broken DB on the next schema bump until a numbered migration shipped. Restored to drop-and-recreate; the new `run_migrations` framework is the additive path for tables that should *not* be dropped.
- `refresh_storage` (storage swap at runtime) now runs the same `has_invalid_sqlite_header` pre-flight that `AppState::new` runs at startup. A re-attached USB whose `user_data.db` got clobbered while the Pi was off no longer leaves the pool silently closed — the corruption banner fires and Recovery / Reset is one click away.

### Other

- New tests across the cycle (1 100+ pass total, 18 in `db_pool` alone, 9 in `corruption_tests`):
  - 5 `core-server` unit tests for `scan_systems` paths (`RomsDirUnreadable`, `AllSystemsMissing`, populated, empty-but-readable, missing) and `wait_for_storage_ready`.
  - 5 SQL-level zero-overwrite-guard tests on `save_system_meta`.
  - 3 `ManifestImportStats` serde / back-compat / rate-limit-flag tests.
  - 4 `SsrSnapshot<T>` unit tests including the 10-racer single-flight coalescing test.
  - 7 new `DbPool` property tests covering the WAL-unlink regression (`concurrent_writes_visible_to_all_readers`, `reset_to_empty_blocks_until_drain`, `crash_recovery_simulation`, `gate_blocked_read_returns_typed_error`, `closed_pool_try_read_returns_typed_error`, `corrupt_pool_try_read_returns_typed_error`, `wal_writes_do_not_block_concurrent_reads`).
  - `rebuild_corrupt_library_wipes_table_content` integration test asserting a sentinel row inserted before `mark_corrupt` is gone after the rebuild — content-survival check that catches refactors which would no-op the file wipe and just flip the flag.
  - 11 storage-id unit tests (deterministic derivation from FS UUID, NFS shape, kind-hex format validation, parse round-trip, generate-collision sanity).
  - 22 e2e cases in `test_page_health.py` (route content health, navigation budgets, force-refresh resilience, server-fn registration).
  - 3 e2e cases in `test_response_cache.py` pinning the new `RESPONSE_TTL`.
- Live validated against a Pi 4 + USB+exFAT (DELETE journal, no WAL): `/favorites` after a 12 s pause went from 112 ms (curl) / 173 ms (Playwright SPA navigation) to 28 ms / 77 ms after the response-cache TTL change. WAL-unlink fix verified on a Pi 5 + ext4 SD by repeatedly triggering Rebuild + Update Thumbnails — `get_roms_page nintendo_snes` returns 7 231 / `get_library_summary` returns 23 666 / 21 systems with zero `(deleted)` `library.db-wal` fds throughout.
- Architecture docs (`docs/architecture/connection-pooling.md`, `design-decisions.md`, `database-schema.md`, `technical-foundation.md`) updated to current state.
- WAL-unlink regression analysis in `replay-control-private/investigations/2026-05-01-library-wal-unlink-under-live-connections.md` (the seven independent data-loss vectors and the safety-by-design redesign that closes them). Pool design / cancellation-orphan analysis in `2026-04-29-pool-design-findings.md`. NFS race investigation in `2026-04-29-nfs-startup-race-and-thumbnail-silent-failure.md`. SSR-cache-snapshot proposal in `2026-04-29-ssr-cache-snapshot-vs-pool-starvation.md`.

---

## [0.4.0-beta.4](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.4) - 2026-04-25

### Highlights

- A torn-write or clobbered library database no longer crash-loops the service. Rebuildable caches recover silently on the next start; if your saved overrides and videos are affected, a banner appears with a one-click **Reset** (renamed from Repair).
- Auto-update no longer leaves browsers stuck in a reload loop. After the service restarts, open tabs cleanly pick up the new version on their own.
- Corruption banners now appear instantly instead of after a few seconds of polling, and stale browser tabs reconnect on their own after a server restart.
- Smaller fixes: the captures lightbox no longer crashes when navigating away mid-keypress.

### Added

- Corruption status now pushes over `/sse/config` instead of being polled. Pool-flag transitions broadcast on the existing config stream (init payload + push events); banners read from context `RwSignal`s fed by `SseConfigListener` and a new `SseActivityListener`. The `get_corruption_status` server fn is removed. A new `sqlite::has_invalid_sqlite_header` pre-flight survives torn-write magic-header damage so a clobbered DB no longer crash-loops the service via systemd: `LibraryDb::open` silently delete-recreates (rebuildable cache, no banner), and `user_data` wires through new `DbPool::new_corrupt` so the recovery banner appears via the SSE init payload. `check_for_corruption` now also flags `SQLITE_NOTADB (26)` alongside `SQLITE_CORRUPT (11)`. The user-data "Repair" button is renamed to "Reset".
- Content-hashed WASM and JS asset filenames break the browser cache cleanly across server restarts. `LeptosOptions` sets `hash_files` and reads `hash.txt` from the resolved site root; `build.sh` and `dev.sh` hash the bundle, write `hash.txt`, and rewrite the wasm import inside the JS so wasm-bindgen still resolves. `/static/pkg` now sends `Cache-Control: immutable` since URLs are versioned. Fixes an update-reload loop where the cached pre-restart WASM hydrated with the old `VERSION`, the SSE init reported a mismatch, `location.reload()` re-fetched the same cache, and the loop repeated.
- `build.sh` gains `SKIP_DATA=1` to skip catalog rebuilds for fast iterative WASM-only test builds.

### Changed

- Renamed the on-storage `metadata.db` to `library.db` and folded the grab-bag `metadata::` module into the existing `library::` module across both `replay-control-core` and `replay-control-core-server`. The old module name was a holdover from before the catalog migration — with `catalog.sqlite` now owning embedded reference data, `library.db` clearly names the user's on-storage rebuildable DB.
- Reorganized the former `metadata::` grab-bag into purpose-scoped submodules: `library/db/` (SQLite), `library/imports/` (LaunchBox XML), `library/matching/` (pure alias + metadata matching), `library/thumbnails/` (manifest, fuzzy match, resolution), `library/manuals/` (game docs + retrokit). Hoisted `user_data_db` to its own top-level `user_data/` module (persistent user data is semantically distinct from rebuildable library data). Moved shared SQLite helpers from `metadata/db_common.rs` to top-level `src/sqlite.rs`.
- Renamed the `metadata` cargo feature on `replay-control-core-server` to `library`. Renamed the `metadata_report` bin to `library_report` (`cargo run --bin library_report --features library`). Server fns `rebuild_corrupt_metadata` / `metadata_corrupt` become `rebuild_corrupt_library` / `library_corrupt`; the recovery banner copy reads "Library database is corrupt".
- User-facing "metadata" vocabulary is preserved where it describes external-enrichment sources (the `/settings/metadata` page, the `Game Metadata` i18n label, the `game_metadata` SQL table, the `download_metadata` / `clear_metadata` / `get_metadata_stats` server functions, and `launchbox-metadata.xml`). Only the container DB file and module changed names.
- `DbPool`, `SqliteManager`, and `WriteGate` move from `replay-control-app/src/api/mod.rs` to `replay-control-core-server/src/db_pool.rs`. The types had no app-specific coupling — they just wrap `deadpool-sqlite` around `core-server::sqlite::open_connection` — so SSR consumers now see a single crate for pool + open helpers. App's `api/mod.rs` re-exports `DbPool` / `WriteGate` / `rusqlite` so existing imports keep resolving; `deadpool-*` deps drop out of `replay-control-app`.
- Native I/O for the update system (GitHub release polling, asset download, `available.json` handling) moves from `BackgroundManager` in the app crate into `replay-control-core-server::update` (gated behind the `http` feature). `BackgroundManager` keeps the `AppState` / `Activity` / `systemctl`-coupled orchestration (`update_check_loop`, `start_update*`, `generate_update_script`, etc.). `check_github_update` and `resolve_asset_urls` now take `repo: &str` instead of reading a const.
- Native I/O for the LaunchBox import and thumbnail pipelines extracted into three pure core-server fns: `launchbox::run_bulk_import` (sync XML importer wrapped in `spawn_blocking` with the `Handle::block_on` → `pool.write` bridge for batched flushes), `launchbox::import_launchbox_aliases`, and `thumbnails::update_image_paths_from_disk`. App-side `ImportPipeline::run_import` loses ~50 lines of boilerplate and just wires per-batch ticks into Activity; pipeline ownership stays on the AppState side of the boundary.
- The Axum upload handler delegates filesystem writes to a new `replay_control_core_server::roms::write_rom`, which also creates the system directory if missing. No behavior change.
- `dev.sh` drops the unused `--watch` flag (the Pi auto-redeploy-on-save mode was unused — removed CLI arg, `cargo-watch` loop, and the inline build recipe inside it).

### Fixed

- `CapturesLightbox` keydown listener no longer panics when the page unmounts before the listener detaches. The handler now uses `try_get` on the parent's `current_index` signal so it bails silently on a disposed `RwSignal` instead of unwrapping.
- `setup_checklist` no longer logs a reactive-graph warning on every hydrate. The `query.read().get_str("setup")` call ran in the component body, eagerly reading an `ArcMemo<ParamsMap>` with no tracking context established yet; moved into the `Resource::new` source closure so the read happens inside a tracked context (and the resource now also re-runs if `?setup` is added or removed).
- `SseConfigListener` reconnects after a server restart. The `onerror` handler called `es.close()`, canceling `EventSource`'s built-in retry — stale tabs open during an auto-update therefore never received the fresh init payload that triggers the version-mismatch reload, and silently kept running the previous WASM. Dropped the `onerror` handler so the browser's default ~3s retry kicks in.

### Migration

- Legacy `metadata.db`, `metadata.db-wal`, `metadata.db-shm`, and `metadata.db-journal` files are removed on first boot via an idempotent `cleanup_legacy_metadata_db` step inside `LibraryDb::open`. No data migration is needed: the startup pipeline re-scans ROMs into the new `library.db`, re-imports LaunchBox data from `launchbox-metadata.xml` (Phase 1), and rebuilds the thumbnail index from disk (Phase 3). User overrides and saved videos (`user_data.db`) are untouched.
- `sqlite::delete_db_files` extended to cover `.db-journal`, closing a stale-sidecar gap in the four existing corruption-recovery callers.

### Other

- 7-test lifecycle suite for `DbPool` (read/write roundtrip, closed-pool returns `None`, close-then-read, reopen after close, `mark_corrupt` closing the pool, `WriteGate` RAII guard blocking reads — the last is the exFAT data-corruption guard that justifies the type's existence). Adds tests for `launchbox::run_bulk_import` (covers the `spawn_blocking` + `Handle::block_on` async bridge), `launchbox::import_launchbox_aliases`, `thumbnails::update_image_paths_from_disk`, and `roms::write_rom`. Update tests relocate from app to core-server and switch from an in-process axum listener to mockito; 16/16 green via `cargo test --features http -p replay-control-core-server`.
- Shared `test_utils` pub module in `core-server` (`build_library_pool`, `insert_game_library_row`) avoids fixture duplication across launchbox and thumbnails test modules. No feature flag — workspace-internal helpers compile in unconditionally and are LTO-dropped from release binaries. `tempfile` moves from dev-dep to regular dep.
- 8 Rust integration tests, 6 Rust unit tests, and 4 Playwright e2e tests covering the SSE corruption broadcast, recovery server fns, idempotent `mark_corrupt`, clobbered-header startup, the live browser SSE wire, and the library no-crash-loop path. `conftest` switches from `sshpass` to `SSH_ASKPASS` to drop the system dep.
- `bench.sh` discovers the hashed wasm URL from the served HTML so it tracks whatever the deploy is actually serving; pipefail-safe when the server is on a pre-hash build (falls back with a warning). `mock_github.py`'s fake site tarball ships hashed filenames + `hash.txt` so the post-update server can serve the hydration scripts in container/e2e auto-update tests.
- Architecture docs (`connection-pooling.md`, `technical-foundation.md`, `design-decisions.md`, `enrichment.md`, `server-functions.md`) updated for the core-server extraction: `DbPool` / `SqliteManager` / `WriteGate`, update I/O, `run_bulk_import`, `write_rom`, and `update_image_paths_from_disk` now point at `replay-control-core-server`. Stale `api/cache/*` paths swept after the metadata→library rename.
- `DbPool::new` no longer warms deadpool connections via `block_in_place` + `Handle::block_on`. The sync `sqlite::open_connection` warmup already validates the file (used to detect journal mode); the deadpool warmup it then ran caught no error the sync warmup didn't, since `Manager::create` only adds trivial role pragmas (`cache_size`, `query_only`, `wal_autocheckpoint`). Connections now create lazily on first `pool.get()`. The `block_in_place` pattern requires multi-thread runtime and interacts pathologically with thread oversubscription on small CI runners — corruption_tests had to be marked `#[serial]` to dodge a CI hang triggered by it. With the pattern gone, `#[serial]` and the `serial_test` dev-dep are removed; the 8 corruption tests run in parallel again.
- `thumbnails::manifest::import_all_manifests` takes `&DbPool` instead of `&mut Connection`. The thumbnail-pipeline caller drops the `pool.write(|db| Handle::current().block_on(...))` bridge, and the write connection is now only checked out for each repo's SQL trio (source upsert + entries insert + count patch, still atomic in one tx) rather than held across the per-repo GitHub HTTP fetches. Same on-disk behaviour, lower deadpool occupancy.
- `release-plz` config fix.

---

## [0.4.0-beta.3](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.3) - 2026-04-23

### Highlights

- Snappier homepage, log viewer, and game launches under load. Subprocess and database calls that used to block for 1–2 seconds at a time now run asynchronously, and the new connection pool more than doubles homepage throughput.
- Arcade box art now matches games with apostrophes in their names (e.g. "Galaga '88") instead of falling back to a placeholder.
- A failed game launch no longer leaves behind a stale autostart trigger that could fire on the next boot.

### Added

- Async catalog connection pool via `deadpool-sqlite` replaces the single `OnceLock<Mutex<Connection>>` that serialized lookups under load. Adds `prepare_cached` on every hot path, tuned pragmas (`mmap_size=64MiB`, `cache_size=8MiB`, `temp_store=MEMORY`), and batch APIs for the N+1 sites in `favorites`, `related`, `scan_pipeline`, and `search`. Homepage c=10 throughput: **113 → 265 req/s** vs v0.3.0. See `docs/features/benchmarks.md`.
- New workspace crate `replay-control-core-server` holds all native (linux) server-side code — SQLite, filesystem, HTTP, process spawning, XML parsing. `replay-control-core` is now pure and compiles for both native and `wasm32-unknown-unknown`, eliminating all 89 `#[cfg(target_arch = "wasm32")]` attributes that previously stubbed DB/fs/HTTP on WASM.
- `tools/pi-memory.sh` reads `VmRSS` / `VmHWM` / `RssAnon` / `free -m` from the Pi over SSH. `--restart` for a clean idle baseline, `--wait N` for settle time, `--json` for machine-readable output.

### Changed

- 17 wire types (`Favorite`, `RomEntry`, `SystemSummary`, `GameRef`, `ImportProgress`, `VideoEntry`, `GameDocument`, …) promoted to `replay-control-core`. The `app/src/types.rs` mirror layer is gone; adding a field now means editing one definition, not two, and the `#[cfg(feature = "ssr")] pub use` / `#[cfg(not(ssr))]` switches in `server_fns/*.rs` collapse to unconditional imports.
- Subprocess and filesystem calls on the async request path (`df`, `ip`, `journalctl`, `tail`, `systemctl restart`, `launch_game`'s autostart writes) migrated to `tokio::process::Command` / `tokio::fs::*`. Previously each blocked the reactor for 1–2s on every homepage, log-viewer, and game-launch request.
- `install.sh` now respects `CARGO_TARGET_DIR` (same behaviour as `build.sh`); `--local` deploys no longer need a `target/ → $CARGO_TARGET_DIR` symlink.

### Fixed

- `build-catalog` fails loudly when input data files are missing or unreadable, instead of producing a degraded catalog that passes tests but loses rows at runtime.
- Arcade box-art matcher handles apostrophes in display names (e.g., "Galaga '88"); regression test locks this in.
- `launch_game` cleans up its autostart marker via `tokio::fs::remove_file` when `systemctl restart` fails, preventing a stale trigger on the next boot.

### Other

- Pool warmup validates the catalog schema at `init_catalog` time and surfaces a clear error if the file is missing or schemaless. Previously a 0-byte `/catalog.sqlite` left at the systemd CWD would silently break every query and show bare filenames in the UI.
- Local `DpSql(DatePrecision)` newtype in `library_db` carries the `rusqlite::ToSql` / `FromSql` impls without violating Rust's orphan rule (`DatePrecision` stays pure in core).
- `docs/architecture/` updated for the 3-crate layout with a new "Crate split" design-decisions entry. `CLAUDE.md` gains a "Crate boundary" rule listing the deps forbidden in core.
- `docs/features/benchmarks.md` refreshed for v0.4.0 (Pi 5, 2GB, ~23K ROMs). Memory section now shows idle / right-after-load / +60s-settled: peak 189 MB, settled 62 MB within a minute (jemalloc returns cleanly).
- CI self-heal: `build-release.yml` now creates a missing release instead of failing when fired from a pushed tag without one.

---

## [0.4.0-beta.2](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.2) - 2026-04-21

### Highlights

- Game detail pages now show the full release date (e.g. "Aug 31, 2000") whenever the data is precise enough, instead of always showing only the year.
- Changing your region preference re-resolves release dates instantly — no library re-import needed.

### Added

- Per-region release dates with precision: new `game_release_date` side table stores ISO 8601 partial dates (`YYYY` / `YYYY-MM` / `YYYY-MM-DD`) per (system, base_title, region). `game_library` gets `release_date`, `release_precision`, and `release_region_used` mirror columns resolved against the user's region preference, with `idx_release_date_chrono` for indexed range scans.
- TGDB emit in `build.rs` folds region_ids into four buckets (Canada → USA, Korea → Japan) and records per-region precision heuristics. Arcade pipeline (MAME/FBNeo/Naomi) extracts year-only rows from driver metadata. LaunchBox enrichment upgrades to day-precision USA dates via `ON CONFLICT … DO UPDATE WHERE precision_rank` improves.
- `DatePrecision` enum (Year/Month/Day) with `serde` + rusqlite `ToSql`/`FromSql`, usable from both SSR and WASM. `format_release_date(&str, Option<DatePrecision>, Locale)` renders the game detail page through i18n month-short keys instead of hardcoded strings.

### Changed

- Region preference and secondary region preference saves now re-resolve the `game_library` release-date mirror columns in-place — no re-import required.
- `SearchFilter` year range migrated from `substr(release_date, 1, 4)` to lexicographic compare (`release_date >= 'YYYY' AND < '(Y+1)'`) with `saturating_add` for `u16` overflow. Hits the chrono index directly.
- Decade list query reads from `release_date` instead of `release_year`, using `substr(release_date, 1, 3) || '0'` to form decade buckets.

### Fixed

- Game detail page now shows a formatted release date (e.g. "Aug 31, 2000") when day or month precision is available, labeled "Released" instead of "Release Year". Previously always rendered as year-only regardless of the available data.

### Other

- Resolver SQL refactored from 9 correlated subqueries to a single `ROW_NUMBER() OVER PARTITION BY` CTE with row-value `UPDATE`.
- `build.rs` shares `title_utils::base_title` via `#[path]` module include instead of a duplicate `compute_base_title_build()`.
- SG-1000 lookup tests now run against the canonical DAT after adding `Sega - SG-1000.dat` to `scripts/download-metadata.sh` outputs.
- Metadata analysis rule added to `AI_CONTEXT.md`: exclude ROM hacks, translations, homebrew, and aftermarket when measuring source coverage.

---

## [0.4.0-beta.1](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.1) - 2026-04-19

### Highlights

- Metadata page redesigned: six summary cards (Total Games, Enrichment, Systems, Co-op, Year Span, Library Size) and a mobile-friendly per-system accordion replace the cramped 7-column table.
- Summary cards now refresh automatically after import, rebuild, thumbnail update, or clear — no full page reload required.

### Added

- Redesigned metadata page: six library summary cards (Total Games, Enrichment, Systems, Co-op, Year Span, Library Size) plus a per-system accordion list replacing the cramped 7-column table. Mobile-friendly with no horizontal scroll; expanded rows show coverage bars, composition ratios, arcade driver status, and a footer with year range / verified / co-op counts.
- Fast-CI path via `REPLAY_BUILD_STUB=1`: `build.rs` reads small committed fixtures under `replay-control-core/fixtures/` instead of ~180MB of upstream arcade/No-Intro/TGDB/Wikidata downloads. Lint and Test CI jobs use stub mode; a new nightly `test-full` workflow runs the same tests against real data.

### Changed

- `game_series` lookups resolve aliases at read time via a `candidates` CTE (self + canonical + aliases + sibling aliases), replacing the `propagate_series_to_aliases` denormalization helper that was dropped entirely. Single source of truth, no duplicate rows, no O(N²) per-system propagation pass.
- Wikidata series extraction uses `en,mul` label fallback instead of `en,ja`: prevents Japanese labels from leaking into series names while still resolving series whose only English-equivalent representation is Wikidata's curated multilingual default (e.g. Kirby → Q2569953).

### Fixed

- Metadata page summary cards refetch after import / rebuild / thumbnail update / clear-metadata; previously stayed stale until a full page reload.
- `Cache::invalidate` now clears `game_series` and `game_alias` in addition to `game_library`, so rows populated by a previous binary's embedded data don't survive a Rebuild Game Library.
- `scripts/wikidata-series-extract.py` reads the Flycast CSV from its new `data/arcade/` location (was pointing at the pre-move path, silently dropping Flycast/Naomi names from `series.json`).

### Other

- Integration tests no longer hang under parallel execution — `TestEnv` RAII helper replaces the manual `close_state` pattern that was causing `DbPool::close` to contend with in-flight test traffic.
- Rust 1.95 clippy fixes (`sort_by_key` + `Reverse`, `checked_div`, feature-gated atomic imports, collapsible match arm).

---

## [0.3.1-beta.3](https://github.com/lapastillaroja/replay-control/releases/tag/v0.3.1-beta.3) - 2026-04-14

### Other

- fix CHANGELOG.md to include proper per-release sections (was causing release-plz to dump full history into release notes)

---

## [0.3.1-beta.2](https://github.com/lapastillaroja/replay-control/releases/tag/v0.3.1-beta.2) - 2026-04-14

### Other

- replace softprops/action-gh-release with `gh release upload` CLI

---

## [0.3.1-beta.1](https://github.com/lapastillaroja/replay-control/releases/tag/v0.3.1-beta.1) - 2026-04-14

### Fixed

- restore settings sidebar highlight on back-navigation — IntersectionObserver deferred via requestAnimationFrame
- use sshpass for automatic SSH authentication in installer, with SSH_ASKPASS fallback
- send required `force` body param in GetSetupStatus integration test
- update E2E and SSR tests for `/more` → `/settings` page rename

### Other

- chain build-release from release-plz via workflow_call to fix missing binary assets
- add workflow_dispatch trigger to build-release for manual builds

---

## [0.3.0](https://github.com/lapastillaroja/replay-control/releases/tag/v0.3.0) - 2026-04-13

### Highlights

- ROMs with non-standard filenames now display the correct canonical name and box art (~1,105 name fixes, ~1,682 thumbnail fixes), thanks to CRC hash matching.
- Redesigned Settings page with a two-pane layout, scroll-spy sidebar, and five sections.
- A first-run setup checklist on the home page now guides you through LaunchBox metadata import and thumbnail indexing.
- LaunchBox metadata downloads automatically at install time (skip with `--no-metadata`).
- Anonymous usage analytics are enabled by default; opt out from Settings > Privacy.

### Added

- CRC hash-matched display names and thumbnails — ROMs with non-standard filenames now show correct canonical name and box art (~1,105 name fixes, ~1,682 thumbnail fixes)
- redesigned settings page with two-pane layout, scroll-spy sidebar, and five sections
- anonymous usage analytics with opt-out from Settings > Privacy
- first-run setup checklist on home page for LaunchBox metadata and thumbnail index
- LaunchBox metadata download at install time (skip with `--no-metadata`)
- JPG image support and improved box art variant picker with filesystem scan
- local Pi install auto-detection and `--version` flag in installer

### Fixed

- Clear Images now correctly removes `box_art_url` references from the database
- settings sidebar highlight on back-navigation
- reactivity warning in play order navigation (sequel/prequel links)
- silent DB errors in library and enrichment operations now logged
- install script env var positioning for piped commands
- desktop settings layout max-width on inline items

### Performance

- in-memory user preferences cache (skin, locale, region, font size loaded once at startup)

### Other

- HTTP client migration from curl subprocesses to reqwest with shared async client
- settings architecture moved to system-level with SettingsStore abstraction
- game_metadata table schema validation with column count checks and COALESCE upserts
- image matching fixes for JPG symlinks, filesystem media scan, and exFAT stat ordering

---

## [0.2.0](https://github.com/lapastillaroja/replay-control/releases/tag/v0.2.0) - 2026-04-10

### Added

- add "Same as browser" locale option and bilingual locale names
- add cooperative (co-op) play as search filter and game detail field
- add auto-update system (check, download, install, rollback)
- add i18n support with Spanish and Japanese translations
- box art placeholders for games without cover art
- arcade clone fallback + aggressive normalization for box art
- fuzzy manifest matching + fix on-demand download panic
- metadata page streaming SSR with skeleton loaders
- add tracing instrumentation to server functions
- improve organize favorites UX
- skeleton loaders for streaming SSR
- response cache (10s TTL) + query cache for recommendations
- Phase 3 recommendations — Hidden Gems, Similar, Series Spotlight
- Phase 2 recommendations — rotating curated spotlights
- Phase 1 recommendation improvements — smart rotating pills
- change root password from web UI
- update PWA icons with arcade logo
- add rotating gaming icon to top bar
- add system controller icons to game lists and system cards
- improve change cover variant labels and layout
- PWA app shell caching and offline fallback
- add Search tab to bottom nav, system category icons, unfixed header
- graceful startup when storage unavailable + move assets to /static/
- show app version in More page footer, /api/version endpoint, and HTML meta tag
- alternate versions and cross-system sections on game detail page
- resolve 95% of TOSEC CPC duplicate display names
- show TOSEC bracket flag labels in display names
- TOSEC bracket flag classification and duplicate disambiguation
- TOSEC structured tag parsing (year, publisher, side/disk)
- broadcast SSE for skin and storage change notifications
- organize favorites by developer
- runtime SQLite corruption detection with recovery UI
- auto-generate M3U playlists for multi-part TOSEC games
- streaming download progress for LaunchBox metadata import
- developer name normalization for search and grouping
- unified metadata page — SSE rebuild, any_busy signal, on-load resume
- improve driver_status UX + gitignore load test raw files
- genre badges in favorites, CSS cleanup, fix favorites hydration bug
- multi-file ROM management — safe delete, rename restrictions, orphan cascade
- deadpool-sqlite connection pool for concurrent DB reads
- inline delete confirmation for downloaded manuals
- language preferences + manual fixes
- game manuals — in-folder detection + archive.org on-demand download
- share videos across regional variants via base_title
- add GameListItem shared component
- add REST API endpoints for libretro core
- responsive tablet/desktop CSS breakpoints
- parse CommunityRatingCount + weighted top-rated scoring
- developer search UI and game list page
- developer column, search, and game list page backend
- add Named_Titles support and screenshot gallery
- sequel/prequel play order navigation
- restructure More page + declutter game detail
- unify region preferences into single settings section
- show arcade clone siblings as "Arcade Versions" on game detail
- add pull-to-refresh for iOS PWA standalone mode
- concise labels for Other Versions and clippy cleanup
- add Wikidata series data with arcade support
- add game series and cross-name variant relationship system
- add CRC32 hash-based ROM identification for cartridge systems
- add secondary region preference with Strategy C sort order
- add text size toggle (normal/large) to settings page
- add pull-to-refresh for PWA standalone mode
- redesign metadata page layout with embedded DB stats
- add unified GameInfo API with lightweight RomListEntry
- parse developer, release year, and cooperative from LaunchBox XML
- filter non-playable MAME entries, preserve BIOS with flag
- parse MaxPlayers from LaunchBox XML for player count enrichment
- add orphaned image cleanup with manual UI button
- two-tier genre system with genre_group for unified filtering
- block DB operations during game library rebuild, add completion feedback
- auto-detect new/changed ROMs via filesystem watcher
- add is_special flag and genre fallback from LaunchBox
- add is_hack support — filter hacks from variants/dedup, show Hacks section
- parse genre from LaunchBox XML as fallback for baked-in game_db
- add translations section and filter translations from variants/dedup
- add related games section and improve recommendation diversity
- deduplicate recommendations by filtering clones and regional variants
- randomize top rated and "because you love" recommendations
- switch thumbnail indexing from git clone to GitHub REST API
- metadata busy banner and graceful DB unavailability handling
- auto-match metadata for externally added ROMs
- box art swap — pick alternate cover art per ROM
- prevent parallel metadata operations + SSE fixes + git-based thumbnail indexing
- libretro-thumbnails manifest-based pipeline + metadata page redesign
- integrate launch recents tracking into game launch flow
- SSR recommendations with L2 warmup, enrichment, and race condition fixes
- enable recommendations on home page with client-side loading
- persistent SQLite ROM cache (L2) with nolock-first DB open
- favorites/rating recommendations, fix ScummVM dedup
- game recommendations on home page (Phase 1)
- metadata-enriched search (genre, year) and min-rating filter
- word-level fuzzy search, word-boundary scoring, CPU mitigations
- region preference setting on /more page
- megabit size display for cartridge systems, split CSS into modules
- rating display, multiplayer filter, re-match images, git freshness check
- arcade driver badges, favorites filter, image matching improvements
- unified game list patterns, search navigation fixes, hide Alpha Player
- box art on home/favorites, ROM list filters, storage bar, and search fixes
- extended search filters and ROM list filter persistence
- merge Games tab into Home, rename to Games
- user screenshots with lightbox viewer
- game launch with health check recovery
- search icon in top bar, recent searches, random game, and / shortcut
- global search with filters and home page search bar
- game videos with search, inline preview, and multi-API fallback
- responsive image import UX with SSE and cancellable clone
- search, thumbnails, logs page, image import cancel, and UX fixes
- game images, metadata download, and metadata page redesign
- background metadata import with progress, auto-import, per-system coverage
- add game metadata system with LaunchBox import
- unified GameInfo type, skin sync toggle, theme->skin rename
- interactive skin selection and CSS theming fixes

### Fixed

- use i18n key for series position indicator instead of hardcoded format
- clippy warnings (hydrate target) and Docker e2e networking
- resolve clippy warnings and CI artifact path
- remove unused UpdatingPhase variants (clippy dead_code warning)
- correct style.css test URI to match /static/style.css route
- add /style.css endpoint for integration tests
- move ErrorBoundary to route level, fix metadata result messages
- Suspense must wrap ErrorBoundary for non-blocking resources
- organize favorites preview — all combinations, correct labels
- organize preview shows nested folders and uses genre_group
- organize preview uses genre_group instead of raw genre
- developer page reactive signals and URL filter persistence
- URL-encode # in box art paths, fix reactive signal warning
- search filter persistence, highlights, and back button
- search page back button navigation
- pull-to-refresh visible below Dynamic Island on iPhone
- search page width and input height consistency
- prevent DB corruption on exFAT with write gate
- enrichment reads filenames from L2 instead of L1 cache
- startup pipeline detects incomplete scans, improved cache clarity
- detect external skin changes from replay.cfg and broadcast SkinChanged
- include clone entries in display name disambiguation
- use 1h cache for pkg assets (no content hash in filenames)
- show system display name in startup scanning banner
- show phase, system, and progress count in rebuild banner
- use read_untracked for system display name in favorites page
- unfavorite from any page, recursive search, mtime sort
- resolve all clippy warnings
- add pool timeout and increase DELETE mode readers to 3
- use deadpool async API to prevent tokio worker starvation
- add CSS for rebuild progress text inside action card
- tablet text overflow — hero titles wrap, scroll cards 2-line clamp
- simplify skin change to page reload, fix disabled cursor
- move Clear Downloaded Images to Advanced section
- move 9 write operations from read pool to write pool
- explicit WAL checkpoints after bulk writes, scanning flag for ROM lookups
- filesystem-aware SQLite journal mode — WAL only on POSIX filesystems
- check server busy state before starting metadata/thumbnail operations
- remove hydration mismatch in GameListItem + improve curl_get_json
- batch player lookups to eliminate N+1 in multiplayer filter
- resolve clippy warnings, add path traversal protection, and reduce Closure leaks
- remove param_key Memo causing WASM panic on game navigation
- wrap manual server functions in spawn_blocking + register DeleteManual
- persist skin preference in settings.cfg, not replay.cfg
- resolve code review items — dead code, system display, WhereBuilder
- arcade snap/title resolution via unified resolve_image_on_disk
- prevent tokio worker starvation during image index build
- compact developer search block, arcade box art, query text
- merge developer from LaunchBox metadata into game detail
- use Suspense for game detail to fix sequel link navigation
- clear thumbnail progress after completion
- filesystem-aware SQLite locking + thumbnail auto-rebuild
- review fixes for startup refactoring
- eliminate rogue DB connections causing corruption
- non-blocking startup when game library is empty
- unify box art resolution between cards and detail page
- unify alias resolution with fuzzy matching for colon/dash variants
- metadata page horizontal overflow on mobile
- on-demand thumbnail download panics outside Tokio runtime
- thumbnail download counter starts at 1 instead of 0
- version-stripped box art matching checks fuzzy index too
- prevent orphan cleanup from deleting all images
- path traversal check blocks filenames containing ".."
- resolve Leptos hydration warnings on games page
- guarantee metadata_operation_in_progress is cleared after rebuild
- improve variant labels, filter arcade clones, skip broken symlink previews
- populate rom_cache after import when cache is empty
- stop event propagation on boxart picker close button
- re-enrich rom_cache after metadata/thumbnail imports
- case-ininternal exact matching for thumbnail resolution
- add arcade_db translation for thumbnail matching
- resolve recommendation box art from filesystem
- use fuzzy matching in update_image_paths_from_disk
- invalidate image cache after metadata import
- fall back to log files when journald is disabled
- auto-reopen DB connections when file is deleted externally
- resolve all clippy warnings across codebase
- region preference styling, SSR genres, and box art swap design
- auto-delete image repos after match, add cache management
- keep cloned image repos on disk, add staleness check to Download All
- validate library DB image paths against disk to catch fake-symlink artifacts
- search input focus on client-side navigation, inline genre loading
- revert dropdown arrow to SVG data URI for reliable positioning

### Other

- *(deps)* bump the production group across 1 directory with 3 updates ([#15](https://github.com/lapastillaroja/replay-control/pull/15))
- cache user preferences in memory to avoid per-request file I/O
- move Locale enum to core crate, eliminate hardcoded strings
- apply cargo fmt
- add integration tests for enrichment, schema rebuild, and co-op filter
- extract image resolution, thumbnail pipeline, and search scoring
- apply cargo fmt
- bump app and core version to 0.2.0
- fix clippy warnings across workspace
- split ReplayConfig into SystemConfig and AppSettings
- apply cargo fmt
- update attribution for TGDB developer/publisher/coop/rating data
- reorganize More page into five distinct sections
- *(deps)* bump the production group with 1 update ([#13](https://github.com/lapastillaroja/replay-control/pull/13))
- remove service worker offline support, update dependabot grouping
- Revert "fix: add /style.css endpoint for integration tests"
- update benchmarks to beta.4, rename Pi Configuration
- cargo fmt
- fix clippy warnings — collapsible ifs, dead code, too-many-args
- fix formatting in generate-test-fixtures
- simplify post-refactoring — type alias, Default impl, comments
- restructure cache module — rename, split, simplify
- remove remaining unnecessary #[allow] attributes
- fix clippy warnings and add #[allow] comments
- add skeleton loader CSS for favorites page
- move enrichment pipeline to core crate
- reduce SQLite page cache and read pool to 1 connection
- add jemalloc allocator for better memory management
- remove ImageIndex from request path, use DB box_art_url only
- Revert "fix: organize preview uses genre_group instead of raw genre"
- remove L1 ROM cache — unused after search unification
- review round 2 — GameSection for random picks, fix region format
- simplify review — shared component, remove duplication
- extract shared enrichment, unify GlobalSearchResult into RomListEntry
- use DB box_art_url, skip ImageIndex when possible
- optimize recommendations — eliminate DB round-trip, fix i64 overflow
- unify search backend — single query, shared enrichment
- update dependencies to latest compatible versions
- add license and repository metadata to Cargo.toml files
- unify home search bar with search page input
- add accent-colored logo to top bar, remove system card icons
- use replay.local instead of hardcoded IP, remove stale M3U comment
- fix clippy warnings and remove allow annotations
- extract cache-control header values to constants
- LaunchBoxMetadata tuple to named struct fields
- remove auto M3U generation (should not modify user romset)
- restore blocking SSR for homepage (streaming broke hydration)
- convert activity SSE from polling to broadcast
- SQL pre-filter with search_text column (search 220ms → 14ms)
- parallelize global search across systems via tokio::spawn
- add Cache-Control headers for static assets
- convert homepage to streaming SSR (TTFB 169ms → 7ms)
- limit get_recents to 15 entries (homepage only shows 11)
- apply cargo fmt
- SQL-level pagination for system ROM list
- apply cargo fmt
- single-row DB lookup for game detail pages
- unified Activity enum replacing busy/scanning/rebuild_progress
- unified any_busy signal for metadata page, fix SSE cleanup
- remove is_local from DB layer, use JournalMode enum
- upgrade rusqlite 0.32→0.38, SQLite 3.46.0→3.51.1
- full SSR for all pages — eliminate loading spinner flash
- remove clippy suppressions, extract param structs, consolidate helpers
- remove cache TTL for local storage, extract shared Freshness struct
- split global_search into focused helper functions
- deduplicate SSE handlers with generic sse_progress_stream builder
- extract rom_docs_handler into serve_rom_doc function
- standardize lock expect() messages in import.rs
- deduplicate MEGABIT_SYSTEMS — SSR delegates to core crate
- add integration tests for search helpers, ROM path parsing, and batch player lookup
- add Copy derive to qualifying types
- increase default text size to 110%, large to 140%
- remove all legacy DB Mutex shims, use pool exclusively
- simplify developer query + add 12 tests
- extract reusable hooks and reduce duplication
- remove redundant developer matching from global_search
- limit cargo parallelism to 8 jobs to prevent OOM during builds
- replace RomItem with unified GameListItem across all game lists
- replace remaining tuples with named structs + fix clippy
- cleanup dead code and minor fixes
- extract matching logic to core crate (#2-4)
- unify image matching into single core path
- eliminate hardcoded thumbnail strings across codebase
- consolidate thumbnail logic into core crate
- add Wikidata attribution to metadata page
- unify busy flags, fix startup bugs, per-batch DB locking
- split cache.rs, extract image matching, Arc-wrap ROM cache
- address code review findings — perf, safety, dedup
- sequenced startup pipeline, extract AppState, single DB connection
- split library_db.rs into sub-modules and consolidate utils
- Revert "feat: add pull-to-refresh for PWA standalone mode"
- derive thumbnail counts from game_library.box_art_url
- migrate video storage from videos.json to SQLite user_data.db
- rename rom_cache → game_library across codebase
- move find_image_on_disk and helpers to core crate
- shared DB initialization with eager open and corruption recovery
- replace reqwest with curl for video search API calls
- tier 1+2 optimizations — 98% faster page loads
- remove genre/year from search scoring, add min-rating UI filter
- add integration tests, extract router builder
- SSE metadata progress, .replay-control renames, box art dedup, tests
- extract game_detail sub-components, typed filter state, update docs
- split server_fns.rs and api/mod.rs into domain modules
- extract RebootButton, unify Transition, auto-close SSE stream
- rename log prefix from replay-companion to replay-control
- rename crates to replay-control-app/core, add hostname page, NFS reboot

## 2026-03-30

### Features
- feat: PWA app shell caching and offline fallback — precache static assets (CSS, JS, WASM, icons), cache-first for `/static/`, network-first for navigation, offline error page (`0b34353`)
- feat: add Search tab to bottom nav, system category icons, unfixed header (`cd31b26`)
- feat: graceful startup when storage unavailable + move assets to `/static/` (`e365dbb`)
- feat: add SG-1000 and 32X to baked-in game_db (`2cad33e`)

### Bug Fixes
- fix: startup pipeline detects incomplete scans, improved cache clarity (`7fd46a4`)

### Style
- style: add accent-colored logo to top bar, remove system card icons (`e250af2`)

### Refactoring
- refactor: LaunchBoxMetadata tuple to named struct fields (`d65ba23`)
- refactor: extract cache-control header values to constants (`8e93551`)
- refactor: fix clippy warnings and remove allow annotations (`c338980`)
- revert: remove auto M3U generation — should not modify user romset (`980e2e2`)
- chore: use replay.local instead of hardcoded IP, remove stale M3U comment (`f184151`)

### Documentation
- docs: verify NFS startup v2 design works for all storage types (`b5882e5`)
- docs: mark game detail variant improvements as implemented (`194b3a3`)

---

## 2026-03-27

### Features
- feat: alternate versions section on game detail page — clones and regional variants shown as chip links (`c2f36b9`)
- feat: "Also Available On" cross-system section on game detail page — matches same `base_title` across other systems (`c2f36b9`)
- feat: show TOSEC bracket flag labels in display names — [a] Alternate, [h] Hack, [cr] Cracked, etc. with numbered variants ("Alternate 2", "Trained 3") (`9c9ab13`)
- feat: TOSEC bracket flag classification and duplicate disambiguation — square bracket flags parsed into structured types, used to distinguish otherwise identical display names (`5a34821`)
- feat: TOSEC structured tag parsing — year, publisher, side/disk extraction from TOSEC filenames (`0c4ade8`)
- feat: resolve 95% of TOSEC CPC duplicate display names — version stripping, country codes, bracket flags, format suffix disambiguation (`800515c`)

### Performance
- perf: SQL pre-filter with `search_text` column — search latency 220ms to 14ms (`f79d950`)
- perf: parallelize global search across systems via `tokio::spawn` (`c660635`)
- perf: add Cache-Control headers for static assets (`edaf1df`)
- perf: limit `get_recents` to 15 entries (homepage only shows 11) (`88756b1`)

### Bug Fixes
- fix: default region preference to World instead of USA (`659de9e`)
- fix: fill bidirectional sequel links at build time — reverse-link pass ensures both P155 and P156 are populated (`dbb0b9e`)
- fix: allow clone ROMs as sequel link targets, prefer non-clones (`7c60167`)
- fix: include clone entries in display name disambiguation (`4305d64`)
- fix: use 1h cache for pkg assets — no content hash in filenames, immutable was incorrect (`6c61ee8`)
- fix: show system display name in startup scanning banner (`b48d376`)
- fix: show phase, system, and progress count in rebuild banner (`4dd239c`)
- fix: EU region correctly maps to "Europe" (was "Europe, USA") (`18bfe9f`)

### Refactoring
- refactor: convert activity SSE from polling to broadcast (`598277d`)
- feat: broadcast SSE for skin and storage change notifications (`eb3912d`)

### Documentation
- docs: game detail variant improvements design (`cc5070f`)
- docs: CPC game detail variant coverage analysis (`2852873`)
- docs: TOSEC variant display analysis (`354de38`)
- docs: Discover section redesign with rotating spotlights (`481122d`)
- docs: brainstorm 15 recommendation ideas with priority assessment (`388bdd4`)
- docs: verify TOSEC changes don't break No-Intro parsing (`efce37d`)
- docs: TOSEC structured tag parsing design (`09b1d1a`)
- docs: NFS graceful startup v2 design (`3e08d32`)
- docs: mark sequel/prequel chains as implemented (`83aa121`)
- docs: update load test results, close http-client eval (`bed6c25`)
- docs: TOSEC duplicate analysis and NFS startup v1 (`9f95e48`)

---

## 2026-03-24

### Performance
- perf: async DB pool API (`pool.get().await` + `conn.interact()`) — fixes tokio worker starvation hang that deadlocked the app on game detail pages for large systems (`cf96bf5`)
- perf: pool timeout (10s) + 3 DELETE mode read connections — 3x throughput improvement for Homepage (6.5 → 20.6 req/s at c=5), light endpoints reach 1100+ req/s under mixed load (`6f9df97`)
- perf: single-row DB lookup for game detail pages — 15s → <1ms cold cache by fetching one GameEntry by PK instead of loading all ROMs for the system (`c5d6797`)
- perf: SQL-level pagination for system ROM list — `LIMIT`/`OFFSET` in SQLite instead of loading all rows into memory (`f4f778f`)

### Features
- feat: TOSEC version stripping + country code recognition — improves display names and thumbnail matching for TOSEC-named ROM sets (`18bfe9f`)
- feat: auto-generate M3U playlists for multi-part TOSEC games — detects `(Disc N of M)` / `(Disk N of M)` patterns, groups siblings, writes M3U files at scan time (`7895689`)
- feat: runtime SQLite corruption detection with recovery UI — error-triggered `SQLITE_CORRUPT` detection, per-DB corrupt flag, full-page banner with Rebuild (library.db) or Restore/Repair (user_data.db) options (`1f6aa8c`)
- feat: user_data.db backup at startup — copies healthy DB to `.bak` before background pipeline runs; corruption recovery offers restore from backup (`1f6aa8c`)
- feat: organize favorites by developer — new `Developer` criterion in favorites organize, with `normalize_developer()` handling MAME manufacturer variations (licensing, regional suffixes, joint ventures) (`643bf31`)

### Bug Fixes
- fix: unfavorite from any page when favorites are organized into subfolders — recursive search removes `.fav` from all locations (`5531966`)
- fix: favorites sorted by date added (newest first) instead of system+filename, consistent across subfolders (`5531966`)
- fix: preserve file mtime when copying favorites during reorganization — prevents "Latest Added" showing incorrect results (`bfde961`)
- fix: use `read_untracked()` for system display name in favorites page — fixes reactive tracking warning on WASM hydration (`f0ccd94`)
- fix: startup no longer silently deletes corrupt user_data.db — flags pool and shows recovery banner instead (`1f6aa8c`)

### Code Quality
- fix: resolve all clippy warnings across crates (`f002a9a`)

### Documentation
- docs: update changelog, feature docs, design docs, and known issues for 2026-03-24 changes (`5f48f0d`)

---

## 2026-03-23

### Performance
- perf: full SSR for all pages — `Resource::new_blocking()` + `Suspense` replaces `Transition` on 10 pages, eliminating loading spinner flash; Home 2KB->74KB first paint (`8bfccc6`)
- perf: remove cache TTL for local storage — inotify + mtime + explicit invalidation covers all change scenarios; NFS TTL increased from 5min to 30min (`c6c0aa2`)
- perf: add 4 SQLite indexes (base_title, data_sources_type, series_order, alias_system), optimize `is_empty()` with EXISTS and `delete_orphaned_metadata()` with NOT EXISTS (`1a1a858`)
- chore: upgrade rusqlite 0.32->0.38, SQLite 3.46.0->3.51.1 (`eb5958c`)

### Bug Fixes
- fix: filesystem-aware SQLite journal mode — WAL only on ext4/btrfs/xfs/f2fs; exFAT/FAT32 (USB) get DELETE mode, fixing SQLITE_IOERR_SHORT_READ (522) caused by WAL shared memory incompatibility (`11dc11c`)
- fix: move 9 write operations from read pool to write pool — caught by `query_only = ON` defense on read connections (`3921262`)
- fix: explicit WAL checkpoints after bulk writes, use `scanning` flag instead of `busy` so ROM lookups work during import/thumbnail update (`e1f0fcd`)
- fix: favorites showing empty on reload — replace Transition with Suspense for predictable SSR hydration (`e8e3a8b`)
- fix: check server busy state before starting metadata/thumbnail operations — prevents flash-then-error when another operation is running (`26b6db1`)
- fix: move Clear Downloaded Images to Advanced section — re-downloading all thumbnails is costly, now gated behind Advanced toggle (`1952844`)
- fix: iOS Safari box art rendering (`e8e3a8b`)

### Features
- feat: multi-file ROM delete — enumerates and deletes all associated files (M3U discs, CUE BINs, ScummVM data dirs, SBI companions, arcade CHDs) with file count + total size in confirmation dialog (`445abc9`)
- feat: ROM rename restrictions — block rename for CUE+BIN, ScummVM, binary M3U with reason displayed below actions (`445abc9`)
- feat: orphan cascade on delete/rename — favorites, screenshots, user_data.db (videos, box art), library.db all cleaned up via new `delete_for_rom`/`rename_for_rom` methods (`445abc9`)
- feat: multi-disc detection — `detect_disc_set` finds (Disc N) siblings for Saturn-style CHDs without M3U wrappers (`445abc9`)
- feat: genre badges in favorites cards (`e8e3a8b`)
- feat: improved driver_status UX — hide green "Working" dots (noise for 56% of games), user-friendly labels replacing MAME jargon, "Emulation" heading (`5273f51`)
- feat: production SQLite PRAGMAs — journal_size_limit, foreign_keys, busy_timeout, manual WAL checkpoints on write connections, `query_only` on read connections, hourly PRAGMA optimize, eager pool warmup (`11dc11c`)

### Code Quality
- refactor: remove `is_local` from DB layer, use `JournalMode` enum — DB auto-detects filesystem via `/proc/mounts`, pool sizing based on journal mode (WAL=3 readers, DELETE=1), clean separation from `StorageKind` (`c2abf22`)
- refactor: extract param structs (`FilterUrlParams`, `SystemLookups`, `PaginationParams`) replacing 3 `#[allow(clippy::too_many_arguments)]` (`6aa8661`)
- refactor: consolidate 3 duplicate `format_size` functions into `util::format_size` (`6aa8661`)
- refactor: extract shared `Freshness` struct for cache TTL logic, eliminating duplication across 3 files (`c6c0aa2`)
- style: remove ~50 lines dead CSS, rename `.recent-*` prefix to `.scroll-card-*` (`e8e3a8b`)
- chore: remove sysroot hack — use standard Fedora `dnf --installroot` cross-compile setup with clear setup instructions (`4f28ac2`)

### Documentation
- docs: update for filesystem-aware journal mode, SQLite upgrade, server lifecycle (`04db204`)
- docs: update all documentation for pool migration, ROM management, cache TTL (`9a278c6`)
- docs: add internal analysis documents (`8ccc06c`)
- docs: mark ROM rename cascade as resolved in known issues (`896c927`)
- docs: add cross-compilation reference guide for Fedora (`4f28ac2`)

## 2026-03-22

### Performance
- feat: deadpool-sqlite connection pool — 3 concurrent read connections + 1 write, replacing single Mutex (`2fc1016`, `618314a`)
- fix: batch player lookups to eliminate N+1 in multiplayer filter (`3447489`)
- docs: load test results — 2x throughput for DB-heavy endpoints, 89x for light endpoints under mixed load (`b9d60f9`)

### Bug Fixes
- fix: WASM panic on game detail navigation — ManualSection's param_key Memo triggered effects on disposed signals, freezing the page on "Loading..." (`a009d03`)
- fix: hydration mismatch in GameListItem — removed `#[cfg(ssr)]` system_label resolution that differed between server and client (`9694dd5`)
- fix: path traversal protection on delete_rom/rename_rom server functions (`478f6ec`)
- fix: Closure::forget memory leak in use_debounce — single closure instead of one per keystroke (`478f6ec`)
- fix: SystemTime unwrap → unwrap_or_default in videos.rs (`478f6ec`)

### Code Quality
- refactor: make LibraryDb + UserDataDb stateless query namespaces — methods take `conn: &Connection` (`40072d9`)
- refactor: add Copy derive to 9 qualifying types (`f21652a`)
- refactor: split global_search (295 lines) into focused helper functions (`dbbb2b0`)
- refactor: extract rom_docs_handler from 127-line inline closure in main.rs (`1952b30`)
- refactor: deduplicate SSE handlers with generic sse_progress_stream builder (`ad3968a`)
- refactor: deduplicate MEGABIT_SYSTEMS — SSR delegates to core crate (`dedbe97`)
- refactor: standardize 28 ad-hoc lock expect() messages in import.rs (`be09c8a`)
- fix: resolve 14 clippy warnings across crates (`478f6ec`)
- fix: improve curl_get_json with redirect following, connect timeout, Accept header (`9694dd5`)
- test: integration tests for search helpers, ROM path parsing, batch player lookup (117 tests) (`5678068`)
- style: increase default text size to 110%, large to 140% (`d272648`)

### Documentation
- docs: DB connection pool architecture — design + implementation status (`d01cd23`)
- docs: ROM management analysis — multi-file rename/delete patterns (`a74979a`)
- docs: add scroll restoration to known issues (`a641780`)

## 2026-03-21

- feat: game manuals — in-folder document detection + archive.org on-demand download via RetroKit TSV (`70f1c48`)
- feat: inline delete confirmation for downloaded manuals (`ae8ed86`)
- feat: language preferences for manual search (`e8ab675`)
- fix: wrap manual server functions in spawn_blocking + register DeleteManual (`fe8cdca`)
- fix: 7 correctness + performance fixes for libretro core (`9a9411f`)
- feat: home screen + screensaver design for libretro core (`713c0ff`)
- feat: skin/theme support for libretro core with 11 palette mappings (`203a85b`)
- fix: double-buffered video + UI polish for libretro core (`80c2f3c`)
- feat: multi-page UI, crash fixes, position memory for libretro core (`2cfd599`)
- docs: libretro core skin/theme design (`98c4bb5`)
- docs: RetroAchievements evaluation, core home screen + screensaver designs (`b28f753`)
- docs: update feature documentation through 2026-03-20 (`78e9731`)

## 2026-03-20

- feat: add Named_Titles support and screenshot gallery — title screen + in-game screenshot displayed as labeled gallery on game detail page (`4c10d4e`)
- feat: add developer column to game_library — populated from arcade_db manufacturer + LaunchBox enrichment (uncommitted)
- feat: developer search in global search — searching "Capcom" returns all Capcom games (score 250) (uncommitted)
- feat: "Games by Developer" search block — horizontal scroll of games by matched developer above regular results, with multi-match ranking (uncommitted)
- feat: "Other developers matching" list — up to 2 additional developer matches shown as tappable links with game counts (uncommitted)
- feat: developer game list page at `/developer/:name` — full game list with system filter chips, infinite scroll, empty state for non-existent developers (uncommitted)
- fix: merge developer from LaunchBox metadata into game detail — enrichment was skipping developer field (`a55119c`)
- fix: "Other developers matching" heading shows original query instead of matched developer name (uncommitted)
- refactor: replace remaining tuples with BoxArtGenreRating, ImagePathUpdate, RomEnrichment structs + fix clippy warnings (`0575040`)
- docs: add tablet landscape layouts to proposal C design (`c2855fb`)
- docs: add title screenshots analysis, developer coverage, expand libretro core feasibility with CRT/HDMI support (`46d0e89`)

## 2026-03-19

- feat: sequel/prequel play order navigation — breadcrumb `← Prev | N/M | Next →` using Wikidata P155/P156 chains with ordinal fallback (`8fbba16`)
- feat: cross-system Wikidata series matching — match library ROMs against all Wikidata entries regardless of platform, fixing games like Metal Slug X (Wikidata: sony_psx, ROM: arcade_fbneo) (`964c601`)
- feat: roman numeral normalization for Wikidata matching — "streets of rage ii" now matches "streets of rage 2" (`a04f9f3`)
- fix: correct 4 bogus Wikidata platform QIDs and add 17 missing platforms — DS, PCE, Sega CD, 32X, Atari, 3DO, CD-i, MSX, CPS-3, NAOMI 2, Model 3, ST-V, Neo Geo variants; series data 3,935 → 5,345 entries (+36%) (`e8767b3`)
- fix: exclude only current game from series siblings, not cross-system ports — same game on other systems shows in series, current ROM does not (`ae40730`)
- fix: use Suspense for game detail to fix sequel link navigation — Transition showed stale content making sequel links appear broken (`94e0188`)
- refactor: replace tuple types with AliasInsert/SeriesInsert structs, removing clippy type_complexity warnings (`964c601`)
- chore: cleanup dead code — gate test-only methods behind #[cfg(test)], remove debug eprintln (`a327837`)

## 2026-03-18

- refactor: extract matching logic to core crate — alias_matching, metadata_matching, image_matching modules (`2d9bb6d`)
- refactor: unify image matching into single core find_best_match path (`7f34fc4`)
- refactor: eliminate hardcoded thumbnail strings across codebase (`daedc01`)
- refactor: consolidate thumbnail logic into core crate (`968e051`)
- feat: restructure More page into Preferences / Game Data / System sections + declutter game detail (`e648264`)
- feat: unify region preferences into single settings section (`db0f673`)
- fix: subtitle-stripped fallback for Wikidata series matching — catches DonPachi II and 10+ additional series (`8de96fb`)
- fix: base_title tilde inside parens + enable arcade Wikidata series — 546 arcade entries now populate (`4866c18`)
- docs: add arcade thumbnail gaps + clone series analyses (`1d49dc4`)
- docs: update UI design proposals with new features (`35c99b4`)
- docs: add Wikidata attribution to metadata page (`670c886`)

## 2026-03-17

- refactor: sequenced startup pipeline replacing 4 independent racing tasks with ordered phases — auto-import → populate → enrich → watchers (`5a7abc8`)
- refactor: extract ImportPipeline + ThumbnailPipeline from AppState with shared busy flag for mutual exclusion (`5a7abc8`)
- feat: non-blocking startup — server responds immediately with empty data during warmup, "Scanning game library..." banner shown (`5a7abc8`)
- fix: single DB connection policy — import holds Mutex directly, eliminated 3 rogue LibraryDb::open() calls causing SQLite corruption (`f38f77a`)
- fix: filesystem-aware SQLite locking — WAL mode on local storage (USB/exFAT, SD/ext4), nolock+DELETE on NFS only (`257831f`)
- feat: auto-rebuild thumbnail index at startup when data_sources exists but index is empty (data loss recovery) (`257831f`)
- feat: single-pass LaunchBox XML parsing — was triple-parse taking 15min on Pi, now ~6s (`5a7abc8`)
- fix: remove 10-second cleanup thread delays — busy flag clears immediately after operations (`5a7abc8`)

## 2026-03-16

- feat: add game series and cross-name variant system — algorithmic series_key, TGDB alternates, LaunchBox alternate names (`0ff81d2`)
- feat: add Wikidata series data with arcade support — 3,935 entries across 194 series via SPARQL extraction (`63c07fa`)
- fix: unify alias resolution with fuzzy matching for colon/dash variants — bidirectional TGDB aliases (`a18d9a6`)
- feat: concise labels for "Other Versions" — region only for same-name, name+region for cross-name (`ed40b2c`)
- feat: add CRC32 hash-based ROM identification for cartridge systems — 9 systems with No-Intro DAT matching (`07e9815`)
- feat: add secondary region preference with Strategy C sort order — Primary > Secondary > World (`84879df`)
- feat: add text size toggle (normal/large) with rem-based image scaling (`8951b19`)
- feat: add pull-to-refresh for iOS PWA standalone mode — PullToRefresh.js lazy-loaded (`c53b6f9`)
- feat: show arcade clone siblings as "Arcade Versions" on game detail page (`8ca1cf2`)
- fix: unify box art resolution between cards and detail page — single resolve_box_art() path (`fa14928`)
- refactor: split library_db.rs (2,895 lines) into 7 focused sub-modules (`84cf3d5`)
- fix: tilde dual-title boxart matching — split on ~ and match either half (`84cf3d5`)
- fix: non-blocking startup when game library is empty (`f55ed74`)
- fix: eliminate rogue DB connections causing corruption (`f38f77a`)
- docs: add internal analysis and planning documents (`various`)

## 2026-03-14

- fix: metadata page horizontal overflow on mobile — system names wrap instead of truncating (`61226ab`)
- fix: on-demand thumbnail download panics outside Tokio runtime, breaking enrichment and thumbnail counts after rebuild (`ac36347`)
- fix: thumbnail download counter starts at 1 instead of 0 (`170f638`)
- feat: redesign metadata page layout with embedded DB stats — reorder sections, add built-in game data info card (`d0b2349`)
- feat: add unified GameInfo API with lightweight RomListEntry for ROM list views (`2adcf2b`)
- feat: parse `<Developer>`, `<ReleaseDate>`, `<Cooperative>` from LaunchBox XML (`68b267b`)
- feat: filter non-playable MAME entries at build time, preserve 26 BIOS with `is_bios` flag — arcade DB 28,593 → 15,440 entries (`adf12a2`)
- fix: version-stripped box art matching checks fuzzy index too — fixes Dreamcast TOSEC-named ROMs (`7af0a5f`)
- docs: add player count improvement analysis (`081ae64`)
- feat: parse `<MaxPlayers>` from LaunchBox XML for player count enrichment of 11 zero-coverage systems (`0e1bdd7`)
- refactor: derive thumbnail counts from `game_library.box_art_url` instead of stale `game_metadata.box_art_path` (`0529f8d`)
- fix: prevent orphan cleanup race condition with `metadata_operation_in_progress` guard, skip unenriched systems, 80% safety net (`3645623`)
- docs: add coverage snapshot and non-playable entry analysis (`76ed3f3`)
- feat: add orphaned image cleanup button on metadata page with `find_orphaned_thumbnails()` and `delete_orphaned_metadata()` (`6a522ce`)
- fix: path traversal check `path.contains("..")` → `path.split('/').any(|s| s == "..")` — restores 25 ROM images across 7 systems (`fe253cd`)
- feat: update catver.ini to v0.285 (merged with category.ini, 49,801 entries) and add nplayers.ini v0.278 as player count fallback (427 fills) (`4cddf36`)
- feat: improve image matching with slash dual-name, TOSEC version strip, and CHD filtering (`04ffb89`)
- refactor: consolidate LaunchBox platform mappings into System struct (`2eeea32`)
- feat: improve ScummVM detection and filter orphan M3U stubs (`8c89834`)
- docs: reorganize documentation structure (`9ad58c7`)
- feat: two-tier genre system with `genre_group` for unified filtering (`6afaafc`)
- refactor: migrate video storage from `videos.json` to SQLite `user_data.db` (`6927907`)
- docs: add conventional commits style guideline to CONTRIBUTING.md (`523ce2b`)
- docs: add chronological changelog with commit references (`bf3e91f`)
- fix: resolve Leptos hydration warnings on games page (`a2dfedc`)
- fix: guarantee `metadata_operation_in_progress` is cleared after rebuild, even on panic (`f5c16f8`)
- feat: block DB operations during game library rebuild with completion feedback (`ec47b6d`)
- refactor: rename `rom_cache` → `game_library` across codebase (`412793b`)
- test: fix broken tests and add coverage for is_special, variants, is_local (`cdd250e`)
- fix: improved variant labels, filtered arcade clones, skip broken symlink previews (`5be5e06`)
- feat: auto-detect new/changed ROMs via inotify filesystem watcher on local storage (`5bec806`)

## 2026-03-13

- feat: `is_special` flag to filter FastROM patches, unlicensed, homebrew, pre-release, and pirate ROMs (`9a29b96`)
- feat: `is_hack` support — filter hacks from variants/dedup, show in dedicated Hacks section (`fdbd788`)
- fix: metadata stats use LEFT JOIN with game library fallback for M3U dedup (`54ced4f`)
- feat: app-specific config file (`.replay-control/settings.cfg`) separate from `replay.cfg` (`9a29b96`)
- fix: populate game library after import when cache is empty — startup race condition (`309b8e4`)
- feat: genre fallback from LaunchBox when baked-in game_db has no genre (`f36b6b9`)
- fix: prioritize primary ROMs over betas for genre assignment in build (`89e4410`)
- feat: translation detection and filtering from variants/dedup with dedicated Translations section (`6a503d6`)
- fix: stop event propagation on boxart picker close button (`55a2cd6`)
- feat: related games section with genre-based similarity (`3ef8199`)
- fix: re-enrich game library after metadata/thumbnail imports (`fa76dcc`)
- fix: trailing article normalization in `base_title` for variant grouping (`5262c66`)
- feat: deduplicate recommendations by filtering clones and regional variants (`68f8938`)
- refactor: organize core crate into logical subdirectories (`4b14f20`)
- fix: case-insensitive exact matching for thumbnail resolution (`bb8391c`)
- fix: M3U dedup metadata stats, MAME/FBNeo fallback, PSX m3u extension (`e5e2426`)
- feat: randomize ordering for top-rated and favorites-based recommendation picks (`f46514f`)
- test: arcade image matching pipeline tests (`74e571e`)
- fix: arcade DB translation for thumbnail matching (`a36a6fe`)
- fix: resolve recommendation box art from filesystem (`acbf4d5`)
- fix: fuzzy matching in `update_image_paths_from_disk` (`48912cf`)
- fix: invalidate image cache after metadata import (`b1fd6e1`)
- feat: switch thumbnail indexing from git clone to GitHub REST API (`f7e2438`)
- fix: fall back to log files when journald is disabled (`a943c8c`)

## 2026-03-12

- feat: metadata busy banner and graceful DB unavailability handling (`a702a1d`)
- feat: NVMe storage support for Pi 5 PCIe (`1cee7eb`)
- refactor: shared DB initialization with eager open and corruption recovery (`83654d0`)
- fix: recommendations biased toward systems with downloaded thumbnails (`94675b0`)
- fix: eager DB open with auto-reopen on external file deletion (`b69ff78`)
- fix: filter out stub thumbnails (<200 bytes) during indexing (`6dac291`)
- fix: M3U Windows backslash paths and comma-inverted display names (`ef3258d`)
- feat: auto-match metadata for externally added ROMs using normalized title index (`bf66440`)
- feat: box art swap — pick alternate cover art per ROM from region variants (`abe23ac`)
- style: resolve all clippy warnings across codebase (`5c27f7f`)
- fix: region preference styling, SSR genres, and box art swap design (`cb85f8c`)
- feat: prevent parallel metadata operations with atomic guard (`701510e`)
- feat: manifest-based thumbnail index stored in SQLite for on-demand downloads from GitHub (`29f175d`)
- feat: enhance `dev.sh` with Pi deployment mode, add `strip=debuginfo` to dev profile (`82ef3ac`)
- feat: recents entry creation on successful launch for immediate home page reflection (`b09c8b6`)
- perf: build optimization with `dev.build-override` opt-level 2 (`acb6c94`)
- refactor: replace `reqwest` with `curl` subprocess for HTTP calls, eliminating 11 TLS crates (`9ffc41e`)
- fix: SSR recommendations with L2 warmup, enrichment, and race condition fixes (`36d4505`)
- feat: persistent SQLite game library (L2 cache) with write-through and `nolock` fallback for NFS (`cd47235`)
- perf: 98% faster page loads via tier 1+2 cache optimizations (`6a4e767`)

## 2026-03-11

- feat: favorites/rating-based recommendations and ScummVM dedup fix (`3385e18`)
- feat: home page recommendation blocks — random picks, top genres, multiplayer, favorites-based, top-rated (`e102987`)
- feat: M3U multi-disc support — hide individual disc files when playlist exists, aggregate sizes (`de13e74`)
- feat: metadata-enriched search using genre and year, min-rating filter (`c075242`)
- feat: word-level fuzzy search matching with word-boundary scoring (`6b76abc`)
- fix: auto-delete image repos after match, add cache management (`449e03c`)
- test: integration tests (50+ tests including 15 integration), extract router builder (`8a0bb34`)
- feat: region preference setting affecting sort order and search scoring (`faa135d`)
- feat: megabit size display for 24 cartridge-based systems, split CSS into 17 modules (`7c385b8`)
- refactor: extract game detail sub-components, typed filter state (`93dc64b`)
- refactor: split server functions and API into domain modules (`efc04b5`)
- refactor: extract reusable components — RebootButton, unified Transition, auto-close SSE stream (`e37ee72`)
- feat: arcade driver status badges, favorites filter, rating display, multiplayer filter (`7ef4564`, `54ceb93`)
- fix: validate library DB image paths against disk to catch fake-symlink artifacts (`49413d9`)
- feat: box art thumbnails on home page and favorites, storage disk usage bar (`1926e53`)
- feat: extended search filters and ROM list filter persistence (`5349b87`)
- refactor: merge Games tab into Home page, rename to Games (`ab1695b`)
- feat: user screenshots gallery with fullscreen lightbox viewer (`138cd3d`)
- feat: game launching on RePlayOS with health check and automatic recovery restart (`6f221e4`)
- fix: search input focus on client-side navigation (`2281faa`)
- feat: search icon in top bar, recent searches, random game button, "/" shortcut (`618cb9c`)
- fix: `.fav` suffix in recently played entries and deduplication (`08b28ad`)

## 2026-03-10

- feat: game videos — search via Piped/Invidious APIs, inline preview, pin/save (`b8145d8`)
- feat: dedicated `/search` page with URL-persisted query params (`b620800`)
- feat: image import with SSE progress streaming and cancel support (`638e026`)
- feat: global cross-system search with genre, driver status, and favorites-only filters (`b3bb571`)
- feat: arcade image support via multi-repo mapping (Atomiswave + Naomi + Naomi 2) (`d46a257`)
- fix: improved arcade LaunchBox matching (`b1d5aa1`)
- feat: game images — per-system image download from libretro-thumbnails (`7c53237`)
- feat: background metadata import with progress tracking, auto-import, per-system coverage (`f13a9f2`)
- feat: LaunchBox XML metadata import with streaming parser and normalized title matching (`1f9b515`)
- refactor: skin sync toggle and theme-to-skin rename (`f4e7cd0`)
- feat: interactive skin selection and CSS theming (`b82964a`)

## 2026-03-09

- feat: hostname configuration with mDNS address update (`a3c8386`)
- feat: skin theming — browse and apply RePlayOS skins, sync app colors to active skin (`f0cb7bf`)
- feat: Wi-Fi configuration page and NFS share settings page (`e3f27a3`)
- feat: favorites organization for grouping by system subfolder (`9311e90`)
- feat: internationalization (i18n) support (`9311e90`)
- feat: dynamic storage detection with config file watcher (SD, USB, NFS) (`f685eef`)
- feat: embedded non-arcade game database (~34K ROM entries across 20+ systems) (`693be18`)
- feat: ROM filename parsing for No-Intro and GoodTools naming conventions (`693be18`)
- feat: install script and aarch64 cross-compilation support (`ab0e032`)
- feat: storage type card and empty state on home page (`780dec8`)
- feat: system display name in ROM list header (`53a30c1`)
- fix: add timestamps to favorites for true "recently added" ordering (`2b7f172`)
- feat: game detail page with system, filename, size, format, and arcade metadata (`43a316a`)
- feat: expanded arcade DB with FBNeo, MAME 2003+, and MAME current — 28,593 entries (`5f78bf9`)
- feat: embedded arcade database (PHF map) with Flycast, Naomi, and Atomiswave data (`b54aab7`)
- feat: unfavorite action on favorites page with `ErrorBoundary` handling (`5f688c6`)
- feat: PWA support with manifest and service worker, in-memory cache layer (`c4f1556`)

## 2026-03-08

- feat: initial project setup — Leptos 0.7 SSR app with WASM hydration, Axum server, client-side routing (`af1d5e9`)
- feat: ROM browsing by system with infinite scroll and pagination (`af1d5e9`)
- feat: per-ROM favorite toggle, rename, and delete with confirmation (`af1d5e9`)
- feat: home page with last played hero card, recently played scroll, and library stats grid (`af1d5e9`)
- feat: favorites page with per-system cards (`af1d5e9`)
- chore: dev script (`dev.sh`) with auto-reload support (`a59c0a2`)
