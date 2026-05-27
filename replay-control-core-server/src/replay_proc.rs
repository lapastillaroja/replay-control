//! Shared `/proc` helpers for inspecting the `replay` process.
//!
//! Both the launch flow (`launch.rs`) and the now-playing detector
//! (`replay-control-app/src/api/now_playing.rs`) need to find the `replay`
//! PID and ask what state the binary is in. They used to disagree on the
//! exclusion set (one missed `avtest`, which loads `avtest_libretro.so` but
//! isn't a game). This module is the single source of truth.

use std::fs;

/// Coarse-grained state of the `replay` binary.
///
/// Three observable conditions: process gone, process alive but no game core
/// in `/proc/<pid>/maps` (menu / frontend / still booting), or process alive
/// with a libretro game core mapped. `Playing` carries the maps text used to
/// reach the decision so callers needing the heap range (now-playing's heap
/// scan) don't have to re-read `/proc/<pid>/maps`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplayState {
    NotRunning,
    Menu { pid: u32 },
    Playing { pid: u32, maps: String },
}

/// Cores that *load a libretro `.so` but aren't a game*. Both the menu
/// frontend (`replay_libretro`) and the A/V test tool (`avtest`) match the
/// `*libretro.so` glob but should be treated as "no active game".
const NON_GAME_CORES: &[&str] = &["replay_libretro", "avtest"];

/// Find the PID of the `replay` process by walking `/proc` and matching
/// `/proc/<pid>/comm`. Returns `None` if the process isn't running, the
/// `/proc` listing fails, or no PID's `comm` reads as `replay`.
fn find_replay_pid() -> Option<u32> {
    let entries = fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid_is_replay(pid) {
            return Some(pid);
        }
    }
    None
}

/// Cheap PID check: returns true iff `/proc/<pid>/comm` reads as `"replay"`.
/// Prefer this over re-walking `/proc` once you have a candidate PID.
fn pid_is_replay(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim() == "replay")
        .unwrap_or(false)
}

/// Returns true iff the given `/proc/<pid>/maps` text contains a libretro
/// core mapping that isn't part of [`NON_GAME_CORES`].
fn maps_have_active_game_core(maps: &str) -> bool {
    maps.lines().any(|line| {
        line.contains("libretro.so") && NON_GAME_CORES.iter().all(|exc| !line.contains(exc))
    })
}

/// Map of libretro core basename (the part before `_libretro.so`) to the set
/// of ROM system folders that core can run. Used by the now-playing detector
/// to reject heap-leaked rom paths whose system the loaded core can't run —
/// e.g. when the user opens the overlay menu and browses the Dreamcast
/// section while a Genesis game is running, the menu allocates `.m3u` paths
/// into the heap that look like ROM strings but aren't the active game.
///
/// Sourced from `investigations/2026-04-07-now-playing-detection.md`
/// (private repo). When new RePlayOS cores ship, add them here — an
/// unmapped core means "don't filter", which keeps the detector working but
/// loses the cross-system protection for that core.
const CORE_TO_SYSTEMS: &[(&str, &[&str])] = &[
    ("fbneo", &["arcade_fbneo", "snk_ng", "snk_ngcd"]),
    ("mame", &["arcade_mame"]),
    ("mame2003_plus", &["arcade_mame_2k3p"]),
    ("flycast", &["arcade_dc", "sega_dc"]),
    (
        "genesis_plus_gx",
        &["sega_smd", "sega_sms", "sega_gg", "sega_sg", "sega_cd"],
    ),
    ("picodrive", &["sega_32x"]),
    ("mednafen_saturn", &["sega_st"]),
    ("pcsx_rearmed", &["sony_psx"]),
    ("snes9x", &["nintendo_snes"]),
    ("fceumm", &["nintendo_nes"]),
    ("mgba", &["nintendo_gba", "nintendo_gb", "nintendo_gbc"]),
    ("mupen64plus_next", &["nintendo_n64"]),
    ("dosbox_pure", &["ibm_pc"]),
    ("scummvm", &["scummvm"]),
    ("cap32", &["amstrad_cpc"]),
    ("alpha_player", &["alpha_player"]),
];

/// Return the ROM systems the currently-loaded libretro core can run, or
/// `None` if the loaded core isn't in our map (or if maps contains multiple
/// non-menu cores due to the known unload-leak — see same investigation
/// doc, §"Stale cores in /proc/{pid}/maps"). `None` is the "don't filter"
/// signal: keep the detector working at the cost of losing cross-system
/// protection for that tick.
pub fn loaded_core_systems(maps: &str) -> Option<&'static [&'static str]> {
    let mut found: Option<&str> = None;
    for line in maps.lines() {
        let Some(path) = line.split_whitespace().last() else {
            continue;
        };
        let Some(basename) = path.rsplit('/').next() else {
            continue;
        };
        let Some(core) = basename.strip_suffix("_libretro.so") else {
            continue;
        };
        if NON_GAME_CORES.contains(&core) {
            continue;
        }
        match found {
            None => found = Some(core),
            Some(prev) if prev == core => {}
            Some(_) => return None, // multiple distinct game cores: leak case
        }
    }
    let core_name = found?;
    CORE_TO_SYSTEMS
        .iter()
        .find(|(name, _)| *name == core_name)
        .map(|(_, systems)| *systems)
}

/// Probe `/proc` for the current state of the `replay` binary.
///
/// Pass `cached_pid: Some(p)` to skip the `/proc` walk when a PID is already
/// known; the cache is re-verified via `/proc/<pid>/comm` before being
/// trusted. Returns the observed state including the (possibly updated) PID.
pub fn current_replay_state(cached_pid: Option<u32>) -> ReplayState {
    let pid = match cached_pid.and_then(|p| pid_is_replay(p).then_some(p)) {
        Some(p) => p,
        None => match find_replay_pid() {
            Some(p) => p,
            None => return ReplayState::NotRunning,
        },
    };
    let maps = match fs::read_to_string(format!("/proc/{pid}/maps")) {
        Ok(m) => m,
        // Process disappeared between PID find and maps read.
        Err(_) => return ReplayState::NotRunning,
    };
    if maps_have_active_game_core(&maps) {
        ReplayState::Playing { pid, maps }
    } else {
        ReplayState::Menu { pid }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maps_line(core_basename: &str) -> String {
        format!(
            "7f0000000000-7f0000100000 r-xp 00000000 b3:02 6621 \
             /opt/replay/cores/{core_basename}_libretro.so\n"
        )
    }

    #[test]
    fn loaded_core_systems_returns_known_core_systems() {
        let maps = maps_line("genesis_plus_gx");
        let got = loaded_core_systems(&maps).expect("known core");
        assert!(got.contains(&"sega_smd"));
        assert!(got.contains(&"sega_cd"));
        assert!(!got.contains(&"sega_dc"));
    }

    #[test]
    fn loaded_core_systems_returns_none_for_unknown_core() {
        let maps = maps_line("brand_new_core_that_does_not_exist");
        assert_eq!(loaded_core_systems(&maps), None);
    }

    #[test]
    fn loaded_core_systems_ignores_menu_and_avtest_cores() {
        // Menu state: only replay_libretro mapped → no game core, return None.
        let maps = maps_line("replay_libretro");
        assert_eq!(loaded_core_systems(&maps), None);
        let maps = maps_line("avtest");
        assert_eq!(loaded_core_systems(&maps), None);
    }

    #[test]
    fn loaded_core_systems_returns_none_when_multiple_distinct_cores_present() {
        // Known leak case: previous core's .so still mapped after switch.
        // We can't tell which is active, so disable filtering.
        let maps = format!("{}{}", maps_line("genesis_plus_gx"), maps_line("flycast"));
        assert_eq!(loaded_core_systems(&maps), None);
    }

    #[test]
    fn loaded_core_systems_handles_same_core_multiple_segments() {
        // Real maps has many lines for the same .so (text, data, bss, etc.);
        // they should all collapse to a single core, not trigger the
        // multi-core leak path.
        let line = maps_line("flycast");
        let maps = format!("{line}{line}{line}");
        let got = loaded_core_systems(&maps).expect("flycast known");
        assert!(got.contains(&"sega_dc"));
    }
}
