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
        find_replay_pid, maps_have_active_game_core, pid_is_replay,
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
            let (next_pid, observation) = tokio::task::spawn_blocking(move || observe(pid_in))
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
    /// the new observation. We deliberately walk the heap on every tick —
    /// in-core game switches (e.g. Sonic 1 → Sonic 2 on `genesis_plus_gx`)
    /// don't change `/proc/<pid>/maps`, so the heap is the only signal.
    /// Cost on the supported cores' ~50 MB heap is ~80–120 ms per tick.
    fn observe(cached_pid: Option<u32>) -> (Option<u32>, Option<Observation>) {
        // Cheap PID check first: re-verify the cached PID via /proc/<pid>/comm
        // before scanning all of /proc. Saves the dirent walk in steady state
        // (which is most of the appliance's life — replay PID rarely changes).
        let pid = match cached_pid.and_then(|p| pid_is_replay(p).then_some(p)) {
            Some(p) => p,
            None => match find_replay_pid() {
                Some(p) => p,
                None => return (None, None),
            },
        };

        let maps = match fs::read_to_string(format!("/proc/{pid}/maps")) {
            Ok(m) => m,
            // Process disappeared between PID find and maps read.
            Err(_) => return (None, None),
        };

        if !maps_have_active_game_core(&maps) {
            return (Some(pid), Some(Observation::Menu { pid }));
        }

        // Game core is mapped but the heap walk may miss the ROM path during
        // a fresh launch or a partial read mid-allocation. Hold the user at
        // Menu rather than dropping to NotRunning so the UI doesn't flap.
        let Some(rom_path) = scan_heap_for_rom_path(pid, &maps) else {
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

    async fn lookup_game_info(
        state: &AppState,
        system: &str,
        filename: &str,
        raw_rom_path: &str,
    ) -> Option<ResolvedGameInfo> {
        let sys = system.to_string();
        let fname = filename.to_string();
        let rows = state
            .library_pool
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

        // Fallback: the heap-walk filename can carry trailing bytes from a
        // partial allocation read ("…zip in cache"). Match the row by
        // longest-prefix on `rom_path` so we still resolve the right entry.
        let rom_path = extract_rom_path(raw_rom_path)?;
        let sys = system.to_string();
        state
            .library_pool
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

    fn scan_heap_for_rom_path(pid: u32, maps: &str) -> Option<String> {
        let heap_line = maps.lines().find(|l| l.contains("[heap]"))?;
        let range = heap_line.split_whitespace().next()?;
        let (start_hex, end_hex) = range.split_once('-')?;
        let start = u64::from_str_radix(start_hex, 16).ok()?;
        let end = u64::from_str_radix(end_hex, 16).ok()?;

        let mut mem = fs::File::open(format!("/proc/{pid}/mem")).ok()?;
        mem.seek(SeekFrom::Start(start)).ok()?;

        // We deliberately scan the whole heap and keep the LAST match rather
        // than early-exiting on the first one. Earlier matches are stale paths
        // from previous games / the menu's recents list; the active ROM lives
        // at the highest address. Cost on the supported cores' ~50 MB heap is
        // ~80–120 ms (chunked reads via this 1 MB buffer; detector RSS stays
        // around 2–3 MB). Fine at the 4 s cadence.
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut offset = start;
        let mut found: Option<String> = None;
        let mut overlap = Vec::new();
        while offset < end {
            let to_read = CHUNK_SIZE.min((end - offset) as usize);
            let slice = &mut buf[..to_read];
            if mem.read_exact(slice).is_err() {
                break;
            }
            if let Some(path) = find_rom_path_with_overlap(&overlap, slice) {
                found = Some(path);
            }
            overlap.clear();
            let keep = OVERLAP_SIZE.min(slice.len());
            overlap.extend_from_slice(&slice[slice.len() - keep..]);
            offset += to_read as u64;
        }
        found
    }

    fn find_rom_path_in_chunk(bytes: &[u8]) -> Option<String> {
        let needle = b"/media/";
        let mut i = 0;
        let mut candidate: Option<String> = None;
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
                    candidate = Some(s.to_string());
                }
                i = end;
            } else {
                i += 1;
            }
        }
        candidate
    }

    fn find_rom_path_with_overlap(overlap: &[u8], current: &[u8]) -> Option<String> {
        if overlap.is_empty() {
            return find_rom_path_in_chunk(current);
        }
        let mut combined = Vec::with_capacity(overlap.len() + current.len());
        combined.extend_from_slice(overlap);
        combined.extend_from_slice(current);
        find_rom_path_in_chunk(&combined)
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
            let found = find_rom_path_with_overlap(overlap, second);
            assert_eq!(
                found.as_deref(),
                Some("/media/usb/roms/arcade_fbneo/00 Clean Romset/sfiii3.zip")
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
    }
}
