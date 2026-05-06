//! Shared `/proc` helpers for inspecting the `replay` process.
//!
//! Both the launch health-check (`launch.rs::check_game_loaded`) and the
//! now-playing detector (`replay-control-app/src/api/now_playing.rs`) need to
//! find the `replay` PID and ask "is a game core mapped right now?". They
//! used to disagree on the exclusion set (the launch check missed `avtest`,
//! which loads `avtest_libretro.so` but isn't a game). This module is the
//! single source of truth.

use std::fs;

/// Cores that *load a libretro `.so` but aren't a game*. Both the menu
/// frontend (`replay_libretro`) and the A/V test tool (`avtest`) match the
/// `*libretro.so` glob but should be treated as "no active game".
pub const NON_GAME_CORES: &[&str] = &["replay_libretro", "avtest"];

/// Find the PID of the `replay` process by walking `/proc` and matching
/// `/proc/<pid>/comm`. Returns `None` if the process isn't running, the
/// `/proc` listing fails, or no PID's `comm` reads as `replay`.
pub fn find_replay_pid() -> Option<u32> {
    let entries = fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_str()?;
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if pid_is_replay(name.parse().ok()?) {
            return name.parse().ok();
        }
    }
    None
}

/// Cheap PID check: returns true iff `/proc/<pid>/comm` reads as `"replay"`.
/// Prefer this over re-walking `/proc` once you have a candidate PID.
pub fn pid_is_replay(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|s| s.trim() == "replay")
        .unwrap_or(false)
}

/// Returns true iff the given `/proc/<pid>/maps` text contains a libretro
/// core mapping that isn't part of [`NON_GAME_CORES`].
pub fn maps_have_active_game_core(maps: &str) -> bool {
    maps.lines().any(|line| {
        line.contains("libretro.so") && NON_GAME_CORES.iter().all(|exc| !line.contains(exc))
    })
}
