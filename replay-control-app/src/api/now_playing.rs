//! Detect which game (if any) is currently running on the appliance.
//!
//! The full validation history, perf numbers, and rejected alternatives live
//! in the private repo: `investigations/2026-04-07-now-playing-detection.md`.
//! Algorithm in brief:
//!   1. Find the `replay` PID via `/proc/*/comm`.
//!   2. Read `/proc/{pid}/maps` to check for a non-menu libretro core.
//!   3. Scan `/proc/{pid}/mem` heap for `/media/.../roms/...` strings.
//!   4. Use the LAST match. Newer heap allocations sit at higher addresses,
//!      and RePlayOS leaks earlier cores' paths on game switch (the menu also
//!      caches recents) — earlier matches are likely stale.
//!
//! Two transient-state defenses live in this module:
//!   - **Debounce**: a state change is only published after two consecutive
//!     polls agree. The ~2 s core-transition window produces truncated/mixed
//!     heap content (the classic "...zip in cache" artefact) that we must
//!     filter out. `NotRunning` is exempt — losing the PID is unambiguous.
//!   - **"Core loaded but path missing" → Menu**: when a game core is mapped
//!     but the heap walk found no path (mid-load, partial read), we report
//!     Menu rather than NotRunning so the UI doesn't flap to "not running"
//!     and back during a launch.
//!
//! The detector is RePlayOS-only: `cfg(target_os = "linux")` gates everything
//! that touches `/proc`. Other targets get a no-op `run_now_playing_loop`.

use std::time::Duration;

use super::AppState;

const POLL_INTERVAL: Duration = Duration::from_secs(4);

#[cfg(target_os = "linux")]
pub async fn run_now_playing_loop(state: AppState) {
    linux::run(state).await;
}

#[cfg(not(target_os = "linux"))]
pub async fn run_now_playing_loop(_state: AppState) {
    // Non-RePlayOS hosts (dev laptops, CI) don't have the /proc layout this
    // detector relies on. Keep the task alive so the spawn site stays uniform
    // but produce no state — `NowPlayingState::NotRunning` is the default.
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;
    use std::io::{Read, Seek, SeekFrom};
    use std::time::{SystemTime, UNIX_EPOCH};

    use replay_control_core_server::db_pool::rusqlite::OptionalExtension;
    use replay_control_core_server::library_db::LibraryDb;
    use replay_control_core_server::replay_proc::{
        ReplayState, current_replay_state, loaded_core_systems,
    };

    use super::{AppState, POLL_INTERVAL};
    use crate::types::NowPlayingState;

    const CHUNK_SIZE: usize = 1024 * 1024;
    const OVERLAP_SIZE: usize = 8 * 1024;

    pub async fn run(state: AppState) {
        let mut session = Session::default();
        let mut last_observation: Option<Observation> = None;
        // PID cache: when set, we re-verify it via /proc/<pid>/comm before
        // falling back to a full /proc walk. This is safe across in-core
        // game switches because the `replay` PID stays the same — only the
        // heap content changes, and we always re-walk the heap.
        let mut cached_pid: Option<u32> = None;

        loop {
            let pid_in = cached_pid;
            let prev_target = session.target.clone();
            let (next_pid, observation) =
                tokio::task::spawn_blocking(move || observe(pid_in, prev_target))
                    .await
                    .unwrap_or_else(|e| {
                        tracing::debug!("now-playing observe task failed: {e}");
                        (None, None)
                    });
            cached_pid = next_pid;

            // Debounce: confirm a state change only when two consecutive
            // observations agree. `None` (PID missing) is exempt — there's no
            // ambiguity to filter and waiting another 4 s would leave the UI
            // stuck on the previous game after a service restart.
            let confirmed = match (&last_observation, &observation) {
                (_, None) => Some(None),
                (Some(prev), Some(curr)) if prev.matches(curr) => Some(Some(curr.clone())),
                _ => None,
            };
            last_observation = observation;

            if let Some(confirmed) = confirmed {
                let next = session.advance(&state, confirmed).await;
                state.set_now_playing(next);
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Per-loop session state: tracks the current PID and active target so we
    /// can tell `same game, same session` (don't reset elapsed) apart from
    /// `service restart` or `game switch` (do reset).
    #[derive(Default)]
    struct Session {
        pid: Option<u32>,
        target: Option<(String, String)>,
        started_at_unix_secs: u64,
    }

    impl Session {
        async fn advance(
            &mut self,
            state: &AppState,
            observation: Option<Observation>,
        ) -> NowPlayingState {
            match observation {
                None => {
                    self.reset();
                    NowPlayingState::NotRunning
                }
                Some(Observation::Menu { pid }) => {
                    if self.pid != Some(pid) {
                        // Service restart: previous session is gone even if
                        // the next observation is still "menu".
                        self.reset();
                    }
                    self.pid = Some(pid);
                    self.target = None;
                    self.started_at_unix_secs = 0;
                    NowPlayingState::Menu
                }
                Some(Observation::Playing {
                    pid,
                    system,
                    filename,
                    rom_path,
                }) => {
                    let resolved = lookup_game_info(state, &system, &filename, &rom_path).await;
                    if resolved.is_none() {
                        tracing::warn!(
                            system = %system,
                            filename = %filename,
                            rom_path = %rom_path,
                            "now-playing: no library match"
                        );
                    }
                    let canonical_filename = resolved
                        .as_ref()
                        .map(|info| info.filename.clone())
                        .unwrap_or_else(|| filename.clone());

                    let new_target = (system.clone(), canonical_filename.clone());
                    let pid_changed = self.pid != Some(pid);
                    let target_changed = self.target.as_ref() != Some(&new_target);
                    if pid_changed || target_changed {
                        self.started_at_unix_secs = now_unix_secs();
                    }
                    self.pid = Some(pid);
                    self.target = Some(new_target);

                    let display_name = resolved
                        .as_ref()
                        .map(|info| info.display_name.clone())
                        .unwrap_or_else(|| canonical_filename.clone());
                    let box_art_url = resolved.and_then(|info| info.box_art_url);

                    NowPlayingState::Playing {
                        system_display: replay_control_core::systems::system_display_name(&system),
                        system,
                        filename: canonical_filename,
                        display_name,
                        box_art_url,
                        started_at_unix_secs: self.started_at_unix_secs,
                    }
                }
            }
        }

        fn reset(&mut self) {
            self.pid = None;
            self.target = None;
            self.started_at_unix_secs = 0;
        }
    }

    #[derive(Clone, Debug)]
    enum Observation {
        Playing {
            pid: u32,
            system: String,
            filename: String,
            rom_path: String,
        },
        Menu {
            pid: u32,
        },
    }

    impl Observation {
        /// Two observations match if they describe the same target on the
        /// same PID. Used as the debounce key — `rom_path` is intentionally
        /// excluded because transient core-transition reads can return the
        /// canonical path with stray bytes appended ("…zip in cache").
        fn matches(&self, other: &Observation) -> bool {
            match (self, other) {
                (Observation::Menu { pid: a }, Observation::Menu { pid: b }) => a == b,
                (
                    Observation::Playing {
                        pid: pa,
                        system: sa,
                        filename: fa,
                        ..
                    },
                    Observation::Playing {
                        pid: pb,
                        system: sb,
                        filename: fb,
                        ..
                    },
                ) => pa == pb && sa == sb && fa == fb,
                _ => false,
            }
        }
    }

    struct ResolvedGameInfo {
        filename: String,
        display_name: String,
        box_art_url: Option<String>,
    }

    /// One detection tick. Returns the (possibly updated) cached PID plus
    /// the new observation. We walk the heap on every tick because in-core
    /// game switches (e.g. Sonic 1 → Sonic 2 on `genesis_plus_gx`) don't
    /// change `/proc/<pid>/maps`, so the heap is the only signal. Cost on
    /// the supported cores' ~50 MB heap is ~80–120 ms per tick.
    ///
    /// `previous` is the (system, filename) of the last confirmed playing
    /// target, threaded in so we can keep it locked when it's still in the
    /// heap candidate set (defends against the overlay-menu leak: the menu
    /// drops other-section rom paths into the heap that look like the active
    /// game). See `select_rom_path` for the full selection rules.
    fn observe(
        cached_pid: Option<u32>,
        previous: Option<(String, String)>,
    ) -> (Option<u32>, Option<Observation>) {
        // `/proc` is authoritative for *whether* a game is running: a mapped
        // non-menu libretro core ⇒ Playing, only the menu core ⇒ Menu, no
        // process ⇒ NotRunning.
        let (pid, maps) = match current_replay_state(cached_pid) {
            ReplayState::NotRunning => return (None, None),
            ReplayState::Menu { pid } => return (Some(pid), Some(Observation::Menu { pid })),
            ReplayState::Playing { pid, maps } => (pid, maps),
        };
        let allowed_systems = loaded_core_systems(&maps);

        // Heap scan for *which* game is running. The walk may miss the ROM path
        // during a fresh launch or a partial read mid-allocation. Hold the user
        // at Menu rather than dropping to NotRunning so the UI doesn't flap.
        let candidates = scan_heap_for_rom_paths(pid, &maps);
        let Some(rom_path) = select_rom_path(
            candidates,
            allowed_systems,
            previous.as_ref().map(|(s, f)| (s.as_str(), f.as_str())),
        ) else {
            return (Some(pid), Some(Observation::Menu { pid }));
        };
        let Some((system, filename)) = parse_system_and_filename(&rom_path) else {
            return (Some(pid), Some(Observation::Menu { pid }));
        };
        (
            Some(pid),
            Some(Observation::Playing {
                pid,
                system,
                filename,
                rom_path,
            }),
        )
    }

    /// Pick the active ROM out of all rom_paths found in the heap. Applies,
    /// in order:
    ///
    /// 1. **Prefix-dedup**: drop a path P if some other path P' is `P + "."
    ///    + ext` (e.g. drop `…/gunlock` when `…/gunlock.zip` is also there).
    ///      These are heap-walk truncation artefacts of the same string.
    /// 2. **Cross-system filter**: if `allowed_systems` is `Some`, drop paths
    ///    whose `<system>` isn't in the loaded core's allowed set. Catches
    ///    overlay-menu leaks when browsing a section that uses a different
    ///    core (Shenmue `sega_dc` leaks when playing a Genesis game).
    /// 3. **Sticky previous target**: if `previous` is still a candidate,
    ///    return it. Defends against same-system menu browse — when playing
    ///    Crazy Taxi 2 on Flycast and the menu highlights Shenmue (also DC),
    ///    both paths are in the heap; without stickiness, frequency could
    ///    occasionally flip.
    /// 4. **Highest count, latest in heap as tiebreaker**: the active core
    ///    references the running game's rom_path multiple times (save paths,
    ///    state, etc.) — menu artefacts appear once or twice. When counts
    ///    tie, the latest-encountered path wins (preserves the historical
    ///    "use the last match" semantics).
    fn select_rom_path(
        mut candidates: Vec<(String, usize)>,
        allowed_systems: Option<&'static [&'static str]>,
        previous: Option<(&str, &str)>,
    ) -> Option<String> {
        // 1. Prefix-dedup.
        let drop: std::collections::HashSet<usize> = candidates
            .iter()
            .enumerate()
            .filter_map(|(i, (p, _))| {
                let prefix = format!("{p}.");
                candidates
                    .iter()
                    .any(|(other, _)| other.starts_with(&prefix))
                    .then_some(i)
            })
            .collect();
        let mut idx = 0;
        candidates.retain(|_| {
            let keep = !drop.contains(&idx);
            idx += 1;
            keep
        });

        // 2. Cross-system filter.
        if let Some(allowed) = allowed_systems {
            candidates.retain(|(p, _)| {
                parse_system_and_filename(p)
                    .map(|(sys, _)| allowed.contains(&sys.as_str()))
                    .unwrap_or(false)
            });
        }

        // 2b. Reject joined search-path strings. MAME stores its rompath as one
        // `;`-separated string (e.g. "…/01 Clones;/media/nfs/bios/mame/roms");
        // the heap walk captures it whole and `parse_system_and_filename` would
        // yield a bogus filename ("roms"). A real ROM path never contains `;`.
        candidates.retain(|(p, _)| !p.contains(';'));

        // 2c. Drop bare extensionless "noise" — a clone's *parent* romset short
        // name ("simpsons" while running "simpsons2p.zip"), or a path fragment.
        // A real ROM file has an extension. An extensionless candidate that is a
        // *game directory* — one other candidates live under, e.g. a ScummVM
        // game folder — is legitimate and is KEPT; dropping it would let an
        // internal file win and degrade ScummVM. Never empty the set. This is a
        // heuristic tiebreak only: it fixes the wrong-variant case but does not
        // resolve multi-disc or ScummVM — the library-aware resolver is the
        // authority for which candidate is the launched game.
        fn has_real_extension(filename: &str) -> bool {
            // Strict: a `.`-suffix of 1..=8 alphanumerics over a non-empty stem,
            // so "Mr. Do" / "v1.000"-style names aren't treated as files.
            filename.rsplit_once('.').is_some_and(|(stem, ext)| {
                !stem.is_empty()
                    && (1..=8).contains(&ext.len())
                    && ext.bytes().all(|b| b.is_ascii_alphanumeric())
            })
        }
        let keep: Vec<bool> = candidates
            .iter()
            .map(|(p, _)| {
                if parse_system_and_filename(p).is_some_and(|(_, f)| has_real_extension(&f)) {
                    return true; // real file → keep
                }
                // extensionless → keep only if it is a directory other
                // candidates live under (a game folder), else it is bare noise.
                let prefix = format!("{p}/");
                candidates
                    .iter()
                    .any(|(o, _)| o != p && o.starts_with(&prefix))
            })
            .collect();
        if keep.iter().any(|&k| k) {
            let mut idx = 0;
            candidates.retain(|_| {
                let k = keep[idx];
                idx += 1;
                k
            });
        }

        if candidates.is_empty() {
            return None;
        }

        // 2d. ScummVM: the heap is dominated by the game's internal data files
        // (SPEECH/*.CLU, MUSIC/*.WAV, …), which vastly outnumber the launched
        // content — so a plain count pick lands on noise (e.g. SPEECH2.CLU). The
        // game is identified by its `.svm`/`.scummvm` content file (whose stem
        // matches the library `.m3u`) or, failing that, the game folder. Prefer
        // those and drop the internal-file noise, so the resolver (which
        // stem-matches ScummVM rows) can name the game. This is a per-system
        // rule; the generic library-aware resolver remains the ideal but needs
        // the async DB path. Other systems are untouched (no ScummVM candidate).
        let is_scummvm =
            |p: &str| parse_system_and_filename(p).is_some_and(|(s, _)| s == "scummvm");
        if candidates.iter().any(|(p, _)| is_scummvm(p)) {
            let is_svm = |p: &str| {
                parse_system_and_filename(p).is_some_and(|(_, f)| {
                    matches!(
                        f.rsplit_once('.')
                            .map(|(_, e)| e.to_ascii_lowercase())
                            .as_deref(),
                        Some("svm") | Some("scummvm")
                    )
                })
            };
            let prefer_svm = candidates.iter().any(|(p, _)| is_svm(p));
            let keep: Vec<bool> = candidates
                .iter()
                .map(|(p, _)| {
                    if !is_scummvm(p) {
                        return true; // leave non-ScummVM candidates alone
                    }
                    if prefer_svm {
                        is_svm(p)
                    } else {
                        // no `.svm` in the heap → prefer the game folder (a
                        // directory the other candidates live under).
                        let prefix = format!("{p}/");
                        candidates
                            .iter()
                            .any(|(o, _)| o != p && o.starts_with(&prefix))
                    }
                })
                .collect();
            if keep.iter().any(|&k| k) {
                let mut idx = 0;
                candidates.retain(|_| {
                    let k = keep[idx];
                    idx += 1;
                    k
                });
            }
        }

        // 3. Sticky previous target.
        if let Some((prev_sys, prev_fname)) = previous
            && let Some((path, _)) = candidates.iter().find(|(p, _)| {
                parse_system_and_filename(p)
                    .map(|(sys, fname)| sys == prev_sys && fname == prev_fname)
                    .unwrap_or(false)
            })
        {
            return Some(path.clone());
        }

        // 4. Highest count, last-in-vec as tiebreaker (heap scan is address-ordered).
        candidates
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(p, _)| p)
    }

    async fn lookup_game_info(
        state: &AppState,
        system: &str,
        filename: &str,
        raw_rom_path: &str,
    ) -> Option<ResolvedGameInfo> {
        let sys = system.to_string();
        let fname = filename.to_string();
        let rows = state
            .library_reader
            .read(move |conn| LibraryDb::lookup_game_entries(conn, &[(&sys, &fname)]))
            .await
            .and_then(|r| r.ok());

        if let Some(rows) = rows
            && let Some(row) = rows.get(&(system.to_string(), filename.to_string()))
        {
            return Some(ResolvedGameInfo {
                filename: row.rom_filename.clone(),
                display_name: row
                    .display_name
                    .clone()
                    .unwrap_or_else(|| row.rom_filename.clone()),
                box_art_url: row.box_art_url.clone(),
            });
        }

        // ScummVM stem fallback. `select_rom_path` step 2d selects one of two
        // shapes for ScummVM, and the exact `(scummvm, <name>)` lookup above
        // misses the `.m3u` row for both:
        //   - the `.scummvm`/`.svm` content file (when it is in the heap) —
        //     strip the extension to get the stem.
        //   - the game *folder* (no-`.svm` path), which has no extension — the
        //     folder name already equals the stem.
        // Its basename stem matches the library `.m3u` stem even when the folder
        // tag differs, so resolve by extension-insensitive stem. `filename_stem`
        // returns `None` on the dot-less folder, so fall back to the name as-is.
        if system == "scummvm" {
            let stem = filename_stem(filename).unwrap_or(filename).to_string();
            let row = state
                .library_reader
                .read(move |conn| LibraryDb::lookup_scummvm_by_stem(conn, &stem))
                .await
                .and_then(|r| r.ok())
                .flatten();
            if let Some(row) = row {
                return Some(ResolvedGameInfo {
                    filename: row.rom_filename.clone(),
                    display_name: row
                        .display_name
                        .clone()
                        .unwrap_or_else(|| row.rom_filename.clone()),
                    box_art_url: row.box_art_url,
                });
            }
        }

        // Fallback: the heap-walk filename can carry trailing bytes from a
        // partial allocation read ("…zip in cache"). Match the row by
        // longest-prefix on `rom_path` so we still resolve the right entry.
        let rom_path = extract_rom_path(raw_rom_path)?;
        let sys = system.to_string();
        state
            .library_reader
            .read(move |conn| {
                conn.query_row(
                    "SELECT rom_filename, display_name, box_art_url
                     FROM game_library
                     WHERE system = ?1
                       AND ?2 LIKE rom_path || '%'
                     ORDER BY LENGTH(rom_path) DESC
                     LIMIT 1",
                    replay_control_core_server::db_pool::rusqlite::params![sys, rom_path],
                    |row| {
                        let filename: String = row.get(0)?;
                        let display_name: Option<String> = row.get(1)?;
                        let box_art_url: Option<String> = row.get(2)?;
                        Ok(ResolvedGameInfo {
                            display_name: display_name.unwrap_or_else(|| filename.clone()),
                            filename,
                            box_art_url,
                        })
                    },
                )
                .optional()
                .map_err(|e| {
                    replay_control_core::error::Error::Other(format!(
                        "lookup now_playing fallback: {e}"
                    ))
                })
            })
            .await
            .and_then(|r| r.ok())
            .flatten()
    }

    /// Scan the full heap and return every `/media/.../roms/...` path found
    /// together with how many times it appears, in address order. The vec is
    /// preserved in scan order so callers can use position as the
    /// last-match tiebreaker.
    fn scan_heap_for_rom_paths(pid: u32, maps: &str) -> Vec<(String, usize)> {
        let Some(heap_line) = maps.lines().find(|l| l.contains("[heap]")) else {
            return Vec::new();
        };
        let Some(range) = heap_line.split_whitespace().next() else {
            return Vec::new();
        };
        let Some((start_hex, end_hex)) = range.split_once('-') else {
            return Vec::new();
        };
        let Ok(start) = u64::from_str_radix(start_hex, 16) else {
            return Vec::new();
        };
        let Ok(end) = u64::from_str_radix(end_hex, 16) else {
            return Vec::new();
        };

        let Ok(mut mem) = fs::File::open(format!("/proc/{pid}/mem")) else {
            return Vec::new();
        };
        if mem.seek(SeekFrom::Start(start)).is_err() {
            return Vec::new();
        }

        // Accumulate occurrences as a Vec to preserve scan order; the
        // last-encountered path of a tied count wins downstream. Cost on the
        // supported cores' ~50 MB heap is ~80-120 ms per tick at the 4 s
        // cadence; detector RSS stays around 2-3 MB.
        let mut order: Vec<String> = Vec::new();
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut offset = start;
        let mut overlap = Vec::new();
        while offset < end {
            let to_read = CHUNK_SIZE.min((end - offset) as usize);
            let slice = &mut buf[..to_read];
            if mem.read_exact(slice).is_err() {
                break;
            }
            for path in find_rom_paths_with_overlap(&overlap, slice) {
                let count = counts.entry(path.clone()).or_insert(0);
                if *count == 0 {
                    order.push(path);
                }
                *count += 1;
            }
            overlap.clear();
            let keep = OVERLAP_SIZE.min(slice.len());
            overlap.extend_from_slice(&slice[slice.len() - keep..]);
            offset += to_read as u64;
        }
        order
            .into_iter()
            .map(|p| {
                let c = counts.get(&p).copied().unwrap_or(0);
                (p, c)
            })
            .collect()
    }

    fn find_rom_paths_in_chunk(bytes: &[u8]) -> Vec<String> {
        let needle = b"/media/";
        let mut out = Vec::new();
        let mut i = 0;
        while i + needle.len() < bytes.len() {
            if &bytes[i..i + needle.len()] == needle {
                let start = i;
                let mut end = i;
                while end < bytes.len() && bytes[end] != 0 && bytes[end] >= 0x20 {
                    end += 1;
                }
                if let Ok(s) = std::str::from_utf8(&bytes[start..end])
                    && s.contains("/roms/")
                    && !s.contains("/_extra/")
                {
                    out.push(s.to_string());
                }
                i = end;
            } else {
                i += 1;
            }
        }
        out
    }

    fn find_rom_paths_with_overlap(overlap: &[u8], current: &[u8]) -> Vec<String> {
        if overlap.is_empty() {
            return find_rom_paths_in_chunk(current);
        }
        let mut combined = Vec::with_capacity(overlap.len() + current.len());
        combined.extend_from_slice(overlap);
        combined.extend_from_slice(current);
        find_rom_paths_in_chunk(&combined)
    }

    fn parse_system_and_filename(path: &str) -> Option<(String, String)> {
        let marker = "/roms/";
        let rest = path.split_once(marker)?.1;
        let (system, tail) = rest.split_once('/')?;
        if system.is_empty() || tail.is_empty() {
            return None;
        }
        let filename = tail.rsplit('/').next()?;
        if filename.is_empty() {
            return None;
        }
        Some((system.to_string(), filename.to_string()))
    }

    fn extract_rom_path(path: &str) -> Option<String> {
        let (_, rest) = path.split_once("/roms/")?;
        Some(format!("/roms/{rest}"))
    }

    /// The basename stem of a filename: everything before the last `.`.
    /// Returns `None` when there is no extension to strip (no dot), so callers
    /// only attempt extension-insensitive matching on files that actually have
    /// an extension. e.g. `"Bargon Attack (CD Spanish).svm"` → `"Bargon Attack
    /// (CD Spanish)"`.
    fn filename_stem(filename: &str) -> Option<&str> {
        filename.rsplit_once('.').map(|(stem, _)| stem)
    }

    fn now_unix_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parse_system_and_filename_handles_subdirs() {
            let p = "/media/usb/roms/arcade_fbneo/00 Clean Romset/sfiii3.zip";
            let got = parse_system_and_filename(p);
            assert_eq!(
                got,
                Some(("arcade_fbneo".to_string(), "sfiii3.zip".to_string()))
            );
        }

        #[test]
        fn parse_system_and_filename_rejects_invalid_path() {
            assert_eq!(parse_system_and_filename("/media/usb/nope"), None);
            assert_eq!(parse_system_and_filename("/media/usb/roms/snes"), None);
        }

        #[test]
        fn filename_stem_strips_last_extension() {
            // ScummVM content files are `.svm`/`.scummvm`; the stem must match
            // the library `.m3u` stem.
            assert_eq!(
                filename_stem("Bargon Attack (CD Spanish).svm"),
                Some("Bargon Attack (CD Spanish)")
            );
            assert_eq!(
                filename_stem("Beneath a Steel Sky (CD DOS Spanish).scummvm"),
                Some("Beneath a Steel Sky (CD DOS Spanish)")
            );
            // Multiple dots: only the last extension is stripped.
            assert_eq!(filename_stem("game.v1.2.svm"), Some("game.v1.2"));
            // No extension: nothing to strip → None.
            assert_eq!(filename_stem("NoExtension"), None);
        }

        #[test]
        fn extract_rom_path_keeps_suffix_for_db_prefix_match() {
            let path = "/media/usb/roms/arcade_fbneo/00 Clean Romset/sfiii3.zip in cache";
            assert_eq!(
                extract_rom_path(path).as_deref(),
                Some("/roms/arcade_fbneo/00 Clean Romset/sfiii3.zip in cache")
            );
        }

        #[test]
        fn finds_rom_path_split_across_chunk_boundary() {
            let prefix = vec![b'x'; CHUNK_SIZE - 5];
            let path = b"/media/usb/roms/arcade_fbneo/00 Clean Romset/sfiii3.zip\0";
            let mut first = prefix;
            first.extend_from_slice(&path[..5]);
            let second = &path[5..];

            let overlap = &first[first.len() - OVERLAP_SIZE..];
            let found = find_rom_paths_with_overlap(overlap, second);
            assert_eq!(
                found,
                vec!["/media/usb/roms/arcade_fbneo/00 Clean Romset/sfiii3.zip".to_string()]
            );
        }

        fn playing(pid: u32, system: &str, filename: &str, rom_path: &str) -> Observation {
            Observation::Playing {
                pid,
                system: system.to_string(),
                filename: filename.to_string(),
                rom_path: rom_path.to_string(),
            }
        }

        #[test]
        fn observation_matches_ignores_rom_path_jitter() {
            let a = playing(123, "snes", "smw.sfc", "/media/usb/roms/snes/smw.sfc");
            let b = playing(
                123,
                "snes",
                "smw.sfc",
                "/media/usb/roms/snes/smw.sfc in cache",
            );
            assert!(a.matches(&b), "rom_path jitter should not break debounce");
        }

        #[test]
        fn observation_does_not_match_across_pid_or_target() {
            let base = playing(1, "snes", "smw.sfc", "/media/usb/roms/snes/smw.sfc");
            assert!(!base.matches(&playing(
                2,
                "snes",
                "smw.sfc",
                "/media/usb/roms/snes/smw.sfc"
            )));
            assert!(!base.matches(&playing(1, "nes", "smw.sfc", "/media/usb/roms/nes/smw.sfc")));
            assert!(!base.matches(&playing(
                1,
                "snes",
                "other.sfc",
                "/media/usb/roms/snes/other.sfc"
            )));
            assert!(!base.matches(&Observation::Menu { pid: 1 }));
        }

        // ── select_rom_path ─────────────────────────────────────────

        const SMD_SYSTEMS: &[&str] = &["sega_smd", "sega_sms", "sega_gg", "sega_sg", "sega_cd"];
        const DC_SYSTEMS: &[&str] = &["arcade_dc", "sega_dc"];

        #[test]
        fn select_drops_prefix_truncation_artefact() {
            // Heap leaks both `gunlock` and `gunlock.zip`; the prefix-dedup
            // should keep only the extended form.
            let candidates = vec![
                ("/media/nfs/roms/arcade_fbneo/gunlock".to_string(), 1),
                ("/media/nfs/roms/arcade_fbneo/gunlock.zip".to_string(), 1),
            ];
            let picked = select_rom_path(candidates, None, None);
            assert_eq!(
                picked.as_deref(),
                Some("/media/nfs/roms/arcade_fbneo/gunlock.zip")
            );
        }

        #[test]
        fn select_filters_cross_system_when_core_allowed_systems_known() {
            // Genesis game running, DC menu leaked Shenmue path. Loaded core
            // is genesis_plus_gx → Shenmue's `sega_dc` is rejected.
            let candidates = vec![
                (
                    "/media/nfs/roms/sega_smd/Sonic & Knuckles.md".to_string(),
                    1,
                ),
                ("/media/nfs/roms/sega_dc/Shenmue.m3u".to_string(), 1),
            ];
            let picked = select_rom_path(candidates, Some(SMD_SYSTEMS), None);
            assert_eq!(
                picked.as_deref(),
                Some("/media/nfs/roms/sega_smd/Sonic & Knuckles.md")
            );
        }

        #[test]
        fn select_picks_highest_count_for_same_system_browse() {
            // DC game running with frequency 4, DC menu-leaked title at
            // frequency 2. Both pass the cross-system filter; count wins.
            let candidates = vec![
                ("/media/nfs/roms/sega_dc/Crazy Taxi 2.gdi".to_string(), 4),
                ("/media/nfs/roms/sega_dc/Shenmue.m3u".to_string(), 2),
            ];
            let picked = select_rom_path(candidates, Some(DC_SYSTEMS), None);
            assert_eq!(
                picked.as_deref(),
                Some("/media/nfs/roms/sega_dc/Crazy Taxi 2.gdi")
            );
        }

        #[test]
        fn select_sticks_with_previous_when_still_present() {
            // Frequencies tie. Without stickiness the latest-encountered
            // wins; with the previous target locked, we keep it.
            let candidates = vec![
                ("/media/nfs/roms/sega_dc/Crazy Taxi 2.gdi".to_string(), 1),
                ("/media/nfs/roms/sega_dc/Shenmue.m3u".to_string(), 1),
            ];
            let previous = Some(("sega_dc", "Crazy Taxi 2.gdi"));
            let picked = select_rom_path(candidates, Some(DC_SYSTEMS), previous);
            assert_eq!(
                picked.as_deref(),
                Some("/media/nfs/roms/sega_dc/Crazy Taxi 2.gdi")
            );
        }

        #[test]
        fn select_abandons_previous_when_no_longer_in_heap() {
            // In-core same-system switch: old ROM path is gone from heap,
            // new one is in. Adopt the new one.
            let candidates = vec![("/media/nfs/roms/sega_smd/Speedball 2.md".to_string(), 1)];
            let previous = Some(("sega_smd", "Sonic & Knuckles.md"));
            let picked = select_rom_path(candidates, Some(SMD_SYSTEMS), previous);
            assert_eq!(
                picked.as_deref(),
                Some("/media/nfs/roms/sega_smd/Speedball 2.md")
            );
        }

        #[test]
        fn select_returns_none_when_filter_kills_everything() {
            let candidates = vec![("/media/nfs/roms/sega_dc/Shenmue.m3u".to_string(), 1)];
            let picked = select_rom_path(candidates, Some(SMD_SYSTEMS), None);
            assert_eq!(picked, None);
        }

        #[test]
        fn select_drops_bare_parent_romset_name() {
            // FBNeo running the clone simpsons2p.zip also leaks the parent
            // romset short name "simpsons" (no extension) and a truncation
            // "simpsons2p". Dedup folds the truncation; the extension
            // preference drops the bare parent so we never report the wrong
            // variant (2P vs the 4P parent family). Regression for the bug
            // that motivated this work.
            const FBNEO: &[&str] = &["arcade_fbneo"];
            let base = "/media/nfs/roms/arcade_fbneo/Horizontal/00 Clean Romset";
            let candidates = vec![
                (format!("{base}/simpsons2p.zip"), 1),
                (format!("{base}/simpsons"), 1),
                (format!("{base}/simpsons2p"), 1),
            ];
            let picked = select_rom_path(candidates, Some(FBNEO), None);
            assert_eq!(picked, Some(format!("{base}/simpsons2p.zip")));
        }

        #[test]
        fn select_rejects_mame_joined_rompath_string() {
            // MAME stores its rompath as one ;-joined string; the heap walk
            // grabs it whole and parse would yield a bogus "roms".
            const MAME: &[&str] = &["arcade_mame"];
            let candidates = vec![
                (
                    "/media/nfs/roms/arcade_mame/Vertical/01 Clones/pacmanblb.zip".to_string(),
                    2,
                ),
                (
                    "/media/nfs/roms/arcade_mame/Vertical/01 Clones;/media/nfs/bios/mame/bios;/media/nfs/bios/mame/roms".to_string(),
                    2,
                ),
            ];
            let picked = select_rom_path(candidates, Some(MAME), None);
            assert_eq!(
                picked.as_deref(),
                Some("/media/nfs/roms/arcade_mame/Vertical/01 Clones/pacmanblb.zip")
            );
        }

        #[test]
        fn select_keeps_extensionless_when_no_extensioned_candidate() {
            // ScummVM-style game folder with no sibling extensioned candidate:
            // the extension preference must not strip the only candidate.
            const SCUMMVM: &[&str] = &["scummvm"];
            let candidates = vec![(
                "/media/nfs/roms/scummvm/Beneath a Steel Sky (CD Spanish)".to_string(),
                2,
            )];
            let picked = select_rom_path(candidates, Some(SCUMMVM), None);
            assert_eq!(
                picked.as_deref(),
                Some("/media/nfs/roms/scummvm/Beneath a Steel Sky (CD Spanish)")
            );
        }

        #[test]
        fn select_prefers_scummvm_svm_over_internal_files() {
            // The Broken Sword case: a real ScummVM heap is dominated by the
            // game's internal data files (SPEECH2.CLU, *.WAV, …) at HIGH counts,
            // plus the game folder and the `.svm` content file at low counts. A
            // plain count pick lands on an internal (e.g. SPEECH2.CLU); the
            // detector must instead pick the `.svm`, which the resolver
            // stem-matches to the library `.m3u`.
            const SCUMMVM: &[&str] = &["scummvm"];
            let base = "/media/nfs/roms/scummvm/Broken Sword 1 - La Leyenda de los Templarios (CD Spanish)";
            let svm =
                format!("{base}/Broken Sword 1 - La Leyenda de los Templarios (CD Spanish).svm");
            let candidates = vec![
                (format!("{base}/SPEECH/SPEECH2.CLU"), 5),
                (format!("{base}/MUSIC/11M2.WAV"), 4),
                (base.to_string(), 2),
                (svm.clone(), 2),
            ];
            let picked = select_rom_path(candidates, Some(SCUMMVM), None);
            assert_eq!(picked.as_deref(), Some(svm.as_str()));
        }

        #[test]
        fn select_scummvm_falls_back_to_game_folder_without_svm() {
            // No `.svm` captured in the heap (partial read): prefer the game
            // folder (a directory the others live under) over the internal
            // files, so the resolver can stem-match the folder.
            const SCUMMVM: &[&str] = &["scummvm"];
            let base = "/media/nfs/roms/scummvm/Flight of the Amazon Queen (CD Spanish)";
            let candidates = vec![
                (format!("{base}/QUEEN.1"), 5),
                (format!("{base}/DATA/THING.DAT"), 4),
                (base.to_string(), 2),
            ];
            let picked = select_rom_path(candidates, Some(SCUMMVM), None);
            assert_eq!(picked.as_deref(), Some(base));
        }
    }
}
