# CPU Impact Analysis: Image Download During Gameplay

Analysis of how the thumbnail download/import process affects gameplay performance on Raspberry Pi, with actionable mitigations.

Last updated: 2026-03-11

## System Context

- **RePlayOS frontend (`/opt/replay/replay`)**: Monolithic C binary that embeds libretro cores via `dlopen()`. Games run in-process (PID 317), not via a separate RetroArch process. Uses DRM/KMS for display, ALSA for audio, SDL2 for input. Runs as `replay.service` under systemd.
- **Companion app (`replay-control-app`)**: Leptos 0.7 SSR app. Runs as `replay-control.service` on port 8080. Uses tokio with default multi-threaded runtime (`#[tokio::main]`).
- **Hardware**: Raspberry Pi 4 (4x Cortex-A72 @ 1.8 GHz) or Pi 5 (4x Cortex-A76 @ 2.4 GHz). Both are 4-core, no SMT.
- **Key constraint**: Both processes share all 4 cores with no CPU pinning or cgroup isolation.

## Operations and Their CPU Cost

### Phase 1: Git Clone (`clone_thumbnail_repo`)

**Code path**: `thumbnails.rs:568-644` -- spawns `git clone --depth 1` as a child process via `std::process::Command`.

| Sub-operation | CPU Profile | Duration |
|---|---|---|
| Network I/O (download packfile) | Low CPU, network-bound | Minutes per repo (varies by size and connection) |
| Packfile decompression (zlib inflate) | **HIGH CPU** -- single-threaded burst | Seconds to tens of seconds per repo |
| Object unpacking and index creation | Moderate CPU | Seconds |
| Working tree checkout (write files to disk) | I/O-bound, low CPU | Seconds |

Repo sizes range from 43 MB (Sega - Naomi 2) to 15 GB (NES, PlayStation, SNES). The zlib decompression of the packfile is the primary CPU concern. Git's `index-pack` process runs single-threaded for `--depth 1` clones and can spike a single core to 100% for several seconds on large repos.

The child process is spawned with default OS scheduling priority (nice 0). It inherits no CPU affinity constraints.

### Phase 2: Fake Symlink Resolution (`resolve_fake_symlinks_in_dir`)

**Code path**: `thumbnails.rs:649-700` -- recursive directory walk, reads every `.png` file under 200 bytes, copies the symlink target over it.

| Sub-operation | CPU Profile | Duration |
|---|---|---|
| Directory traversal (`read_dir` recursion) | Low CPU, syscall-heavy | Depends on file count |
| File metadata checks (`metadata()`, size < 200) | Low CPU | Fast per file |
| Small file reads (checking PNG magic bytes) | Low CPU | Fast per file |
| `fs::copy` for each fake symlink | I/O-bound | Depends on count |

CPU impact: **Low**. This is almost entirely I/O-bound. The number of fake symlinks is typically small (exFAT/FAT32 filesystems only; most USB drives are ext4).

### Phase 3: Fuzzy Index Building (`build_fuzzy_index`)

**Code path**: `thumbnails.rs:207-241` -- reads all filenames from `Named_Boxarts/` and `Named_Snaps/`, builds two `HashMap`s per directory.

| Sub-operation | CPU Profile | Duration |
|---|---|---|
| `read_dir` iteration | Low CPU | Fast |
| String processing (strip suffix, `strip_tags`, `strip_version`, `to_lowercase`) | **Moderate CPU** -- linear in number of entries | Milliseconds to low seconds |
| HashMap insertions | Moderate CPU (hashing) | Scales with entry count |

This runs twice per system (once for boxart, once for snap). Typical entry counts range from hundreds (Naomi 2) to tens of thousands (MAME, NES). For MAME (~5,000 entries x 2 dirs), this takes under a second on Pi hardware. The CPU spike is brief.

### Phase 4: ROM Matching and File Copying (`import_system_thumbnails`)

**Code path**: `thumbnails.rs:297-509` -- iterates every ROM filename, does 3-tier fuzzy lookup, copies matching PNGs.

| Sub-operation | CPU Profile | Duration |
|---|---|---|
| `thumbnail_filename()` per ROM | Negligible | O(n) character mapping |
| `arcade_db::lookup_arcade_game()` per ROM (arcade only) | Negligible | Static phf HashMap lookup |
| `find_thumbnail()` (exact path check + HashMap lookups) | Low CPU | Fast per ROM |
| Colon-variant generation and lookup | Low CPU | 2 extra variants max |
| `copy_png()` / `resolve_fake_symlink()` | I/O-bound | Tens of ms per file |
| `db.bulk_update_image_paths()` (every 10 ROMs) | I/O-bound (SQLite write) | Milliseconds |

CPU impact: **Low to moderate**. The matching logic itself is fast (HashMap lookups). The bottleneck is I/O from `fs::copy` and SQLite writes. On a slow USB drive, this phase is heavily I/O-bound.

### Phase 5: Repo Deletion

**Code path**: `import.rs:488` -- `std::fs::remove_dir_all(&repo_dir)` after successful match.

CPU impact: **Low**. This is a filesystem metadata operation. On ext4, deleting thousands of files involves inode updates but minimal CPU. On exFAT/FAT32, FAT table updates are slower but still I/O-bound. Duration: seconds for large repos.

### Summary: CPU Hotspots

| Phase | CPU Impact | Duration | Cores Affected |
|---|---|---|---|
| Git packfile decompression | **HIGH** | Seconds to tens of seconds | 1 core (100%) |
| Fuzzy index building | Moderate | Sub-second to seconds | 1 core |
| Everything else | Low | Minutes total | 1 core, I/O-bound |

## Impact on Gameplay

### How the RePlayOS Frontend Uses CPU

The `replay` binary runs libretro cores in-process. CPU demand varies dramatically by emulated system:

| System | Core | CPU Demand | Sensitivity to Contention |
|---|---|---|---|
| Atari 2600, NES, Game Boy, SMS | stella, fceumm, mgba, genesis_plus_gx | Low (10-30% of one core) | Low |
| SNES, Genesis, Neo Geo, CPC | snes9x, genesis_plus_gx, fbneo, cap32 | Low-moderate (20-50%) | Low |
| PlayStation, Sega CD | pcsx_rearmed, genesis_plus_gx/picodrive | Moderate (40-80%) | **Medium** |
| N64 | mupen64plus_next | **High** (80-100%+, uses dynarec) | **High** |
| Dreamcast, Saturn, Naomi | flycast, mednafen_saturn | **High** (80-100%+) | **High** |
| DS | melondsds | **High** (multi-threaded) | **High** |
| DOSBox | dosbox_pure | Variable (low to high) | Variable |

### Interaction Model

The companion app's image import runs inside `tokio::task::spawn_blocking`, which executes on tokio's blocking thread pool. The default tokio runtime (`#[tokio::main]`) does not limit the number of blocking threads (up to 512 by default). However, the image import code runs sequentially on a single blocking thread, so only one thread is active.

The git child process (`std::process::Command`) runs as a separate OS process with its own PID. During packfile decompression, this process competes with the `replay` binary for CPU time via the Linux CFS scheduler.

On a 4-core Pi with no CPU pinning:
- Replay binary: typically uses 1-2 cores (main loop + audio thread)
- Git clone decompression: uses 1 core at 100%
- Companion app tokio runtime: uses 1-2 cores (mostly idle, wakes for HTTP requests)

**Worst case**: N64 or Dreamcast game running (2+ cores at near-100%) while git decompresses a large packfile (1 core at 100%). Three of four cores are saturated. The CFS scheduler will fairly distribute time slices, causing:
- **Frame drops** in the emulator (missed vsync deadlines)
- **Audio stuttering** (buffer underruns when the audio callback is delayed)
- **Input lag** (main loop runs slower)

**Best case**: 8-bit/16-bit game running (1 core at 30%) while the image import runs I/O-bound operations. No noticeable impact.

### The Thread Sleep in clone_thumbnail_repo

The cancellation poll loop in `clone_thumbnail_repo()` (line 637) does `std::thread::sleep(200ms)`. This blocks the tokio blocking thread but does NOT affect the git child process or gameplay. This sleep is benign from a CPU perspective -- it simply means the cancellation response latency is up to 200ms.

## Proposed Mitigations

### 1. nice/ionice for the git subprocess (High impact, Low effort)

**What**: Set lower scheduling priority for the git clone child process so the OS scheduler gives the `replay` binary preferential access to CPU time.

**How**: Add `.nice(10)` equivalent before spawning git, and use `ionice` for I/O scheduling.

```rust
// In clone_thumbnail_repo(), before .spawn():
let mut cmd = std::process::Command::new("nice");
cmd.args(["-n", "15", "git", "clone", "--depth", "1", &url, &dest.to_string_lossy()]);
```

Or use the `pre_exec` unsafe hook to call `setpriority()` and `ioprio_set()` directly, which avoids the `nice` wrapper binary:

```rust
use std::os::unix::process::CommandExt;
let mut cmd = std::process::Command::new("git");
cmd.args(["clone", "--depth", "1", &url, &dest.to_string_lossy()]);
unsafe {
    cmd.pre_exec(|| {
        libc::setpriority(libc::PRIO_PROCESS, 0, 15);
        // IOPRIO_CLASS_IDLE (3) << 13
        libc::syscall(libc::SYS_ioprio_set, 1 /*IOPRIO_WHO_PROCESS*/, 0, (3 << 13) | 0);
        Ok(())
    });
}
```

**Effect**: CFS gives the git process smaller time slices when competing with normal-priority processes. With nice 15, the `replay` binary (nice 0) gets roughly 3x more CPU time than git when both are runnable. `IOPRIO_CLASS_IDLE` means git I/O is only served when the disk is otherwise idle.

**Risk**: Slows down the clone operation, potentially significantly on I/O-bound USB drives with `IOPRIO_CLASS_IDLE`. Consider `IOPRIO_CLASS_BE` with priority 7 (lowest best-effort) as a less aggressive alternative.

**Effort**: ~10 lines of code in `thumbnails.rs`.

### 2. nice the companion app's blocking thread (Medium impact, Low effort)

**What**: The fuzzy index building and file copying also run on the companion app's process. Set the blocking thread to a lower nice value.

**How**: At the start of the `spawn_blocking` closure in `import_system_images_blocking`, call `setpriority`:

```rust
tokio::task::spawn_blocking(move || {
    unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 10); }
    // ... rest of import
});
```

**Effect**: The matching/copying work is already mostly I/O-bound, so the impact is modest. But it prevents brief CPU spikes from the fuzzy index building from interfering with the emulator.

**Effort**: 2 lines of code.

### 3. Throttle between file copies (Low impact, Low effort)

**What**: Insert a short sleep between file copy operations to spread I/O load and prevent bus saturation.

**How**: In `import_system_thumbnails`, after each successful `copy_png` or every N iterations, yield:

```rust
// After every 10 ROMs (the progress callback already fires here):
if (i + 1) % 10 == 0 {
    std::thread::sleep(std::time::Duration::from_millis(5));
    // ... existing progress callback
}
```

**Effect**: Adds ~5ms delay per 10 ROMs. For 5,000 ROMs, that is 2.5 seconds total added time. Reduces peak I/O bandwidth competition with the emulator's save-state writes and ROM reads.

**Risk**: Marginal. 2.5 seconds extra on a process that takes minutes is negligible.

**Effort**: 1 line of code.

### 4. Limit git clone bandwidth (Low impact, Low effort)

**What**: Use git's `http.lowSpeedLimit` / transfer settings or the `--shallow-since` option is not applicable here, but we can limit network throughput to reduce CPU load from decompression.

**How**: Not directly feasible via git options for decompression. However, limiting download speed reduces the rate at which data arrives for decompression:

```rust
cmd.args(["clone", "--depth", "1", "-c", "http.lowSpeedLimit=0", "-c", "http.lowSpeedTime=0"]);
```

This does not help. A better approach is the nice value (mitigation 1), which directly addresses the CPU scheduling issue.

**Verdict**: Not recommended as a standalone mitigation. The nice approach is strictly better.

### 5. Detect running game and pause download (High impact, High effort)

**What**: Check if a game is actively running in the `replay` binary and pause the image import.

**How**: The `replay` binary does not expose an IPC mechanism or state file indicating whether a game is running. Possible detection methods:
- Check CPU usage of PID for `replay.service` (> threshold = game running)
- Check `/proc/<pid>/fd` for open libretro core `.so` files other than `replay_libretro.so`
- Check if DRM framebuffer is actively being updated (complex)

**Implementation sketch**:
```rust
fn is_game_likely_running() -> bool {
    // Read /proc/<replay_pid>/stat, check CPU usage
    // Or check open .so files in /proc/<replay_pid>/maps
    // replay_libretro.so = menu (idle), any other *_libretro.so = game running
}
```

The `/proc/<pid>/maps` approach is the most reliable: if any `_libretro.so` other than `replay_libretro.so` is loaded, a game is running. However, this is fragile (depends on internal binary behavior) and may not catch all states.

**Effect**: Would eliminate gameplay interference entirely during active play. Downloads resume when the user returns to the menu.

**Risk**: False positives (user is in menu but a core is still loaded). The `replay` binary hot-swaps cores, so there may be brief windows where a game core is loaded but not running. Also, users may want downloads to continue during gameplay (they walked away and left a game running).

**Effort**: Medium-high. Requires a reliable game-detection mechanism, a pause/resume protocol in the import loop, and testing.

### 6. User-facing warning (Low effort, Good UX)

**What**: Show a notice on the image download page: "Image download may cause brief slowdowns in games running on RePlayOS. For best results, download images when not playing."

**How**: Add an i18n-keyed info notice on the metadata page's image section.

**Effect**: Sets expectations. Users who care about performance can choose when to download.

**Effort**: 1-2 lines in the page template + i18n key.

### 7. CPU affinity pinning (Medium impact, Medium effort, Fragile)

**What**: Pin the git subprocess (and companion app blocking thread) to specific cores, leaving other cores free for the emulator.

**How**: Use `sched_setaffinity` in `pre_exec` to pin git to core 3 only:

```rust
unsafe {
    cmd.pre_exec(|| {
        let mut cpuset: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(3, &mut cpuset);
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &cpuset);
        Ok(())
    });
}
```

**Effect**: Guarantees cores 0-2 are available for the emulator. Git gets only core 3.

**Risk**: The `replay` binary does not pin itself to specific cores. If it also happens to schedule heavily on core 3, this makes things worse. Without knowing the emulator's threading model, CPU pinning can hurt more than help. Also, pinning git to a single core means it cannot parallelize any internal work.

**Verdict**: Not recommended without confirmed knowledge of how the `replay` binary uses cores. The nice approach (mitigation 1) is safer because it lets the OS scheduler make optimal decisions.

## Recommendations (Ranked by Impact/Effort)

| Priority | Mitigation | Impact | Effort | Risk |
|---|---|---|---|---|
| **P0** | nice the git subprocess (nice 15) | High | Low (~10 lines) | Slows clone slightly |
| **P0** | User-facing warning text | Medium (UX) | Low (1-2 lines + i18n) | None |
| **P1** | nice the blocking thread (nice 10) | Medium | Low (2 lines) | None |
| **P1** | ionice for git subprocess (best-effort low) | Medium | Low (part of P0) | May slow clone on slow USB |
| **P2** | Throttle between file copies (5ms/10 ROMs) | Low | Low (1 line) | Negligible time increase |
| **P3** | Detect game running and pause | High | High | Fragile detection, unclear UX |
| **Not recommended** | CPU affinity pinning | Medium | Medium | May worsen performance |
| **Not recommended** | Bandwidth limiting | Low | Low | Does not address the real issue |

## Quantitative Estimates

These are rough estimates based on typical Pi 4/5 performance. Actual impact depends on the specific game, core, and repo size.

| Scenario | Without Mitigations | With P0+P1 (nice) |
|---|---|---|
| 8-bit game + small repo clone | No perceptible impact | No impact |
| 16-bit game + large repo (SNES, 15 GB) | Occasional micro-stutter during decompression (2-5s) | No perceptible impact |
| N64 game + large repo clone | Frame drops, audio glitches for 5-15s during decompression | Mild stutter for 1-2s, extended clone time |
| Dreamcast game + MAME repo (6 GB) | Noticeable frame drops during decompression | Mild impact, clone takes longer |

The P0 mitigation (nice 15 for git) is the single most effective change. It shifts CPU priority so the kernel's CFS scheduler strongly favors the emulator. The cost is that git clone takes somewhat longer (roughly 1.5-2x on a CPU-bound decompression phase), which is acceptable for a background operation.
