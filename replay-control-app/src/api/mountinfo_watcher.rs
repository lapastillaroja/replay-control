//! Event-driven storage detection via `/proc/self/mountinfo`.
//!
//! The kernel signals mount-table changes by waking pollers waiting on
//! `POLLPRI` against `/proc/self/mountinfo` — the canonical event source
//! for "a mount appeared / went away". On non-Linux, [`spawn`] is a
//! no-op and the periodic poll in `spawn_storage_watcher` is the only
//! signal source.

use super::AppState;
use std::path::Path;
use std::time::Duration;

/// Coalesce burst events from `mount` / remount sequences into a single
/// `refresh_storage` call.
const DEBOUNCE: Duration = Duration::from_secs(2);

pub fn spawn(state: AppState) {
    #[cfg(target_os = "linux")]
    spawn_linux(state);
    #[cfg(not(target_os = "linux"))]
    {
        let _state = state;
    }
}

#[cfg(target_os = "linux")]
fn spawn_linux(state: AppState) {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(8);

    // Producer: dedicated thread holding the mountinfo fd. Lives for
    // the process lifetime; tokio's blocking pool would also work but a
    // thread is honest about that.
    std::thread::Builder::new()
        .name("mountinfo-watcher".into())
        .spawn(move || {
            if let Err(e) = run_poll_loop(tx) {
                tracing::warn!("mountinfo watcher exited: {e}");
            }
        })
        .ok();

    tokio::spawn(async move {
        let mut last_mountinfo = read_mountinfo();
        let mut last_active_mount =
            active_storage_mount_signature(&state, last_mountinfo.as_deref());
        while rx.recv().await.is_some() {
            let deadline = tokio::time::Instant::now() + DEBOUNCE;
            loop {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(_)) => continue,
                    Ok(None) => return,
                    Err(_) => break,
                }
            }

            let current_mountinfo = read_mountinfo();
            if current_mountinfo.is_some() && current_mountinfo == last_mountinfo {
                tracing::debug!("mountinfo event but mount table content is unchanged");
                continue;
            }
            if current_mountinfo.is_some() {
                last_mountinfo = current_mountinfo;
            }

            if state.has_storage() {
                let current_active_mount =
                    active_storage_mount_signature(&state, last_mountinfo.as_deref());
                if current_active_mount == last_active_mount {
                    tracing::debug!("mountinfo event but active storage mount entry is unchanged");
                    continue;
                }
                last_active_mount = current_active_mount;
            }

            tracing::debug!("mountinfo: change detected, re-detecting storage");
            // Normal config edits are handled by the replay.cfg watcher. Mount
            // events only need a full config reload for boot recovery, when
            // replay.cfg was unavailable and may have appeared with the SD
            // mount. Re-reading config on every mount churn can report a false
            // "updated" result and unnecessarily restart heavy background work.
            let refresh = if state.has_replay_config() {
                state.redetect_storage().await
            } else {
                state.reload_config_and_redetect_storage().await
            };
            match refresh {
                Ok(true) => tracing::info!("Storage updated after mount-table change"),
                Ok(false) => tracing::debug!("mountinfo event but storage unchanged"),
                Err(e) => tracing::warn!("storage refresh after mount event failed: {e}"),
            }
        }
    });
}

#[cfg(target_os = "linux")]
fn read_mountinfo() -> Option<String> {
    std::fs::read_to_string("/proc/self/mountinfo").ok()
}

#[cfg(target_os = "linux")]
fn active_storage_mount_signature(state: &AppState, mountinfo: Option<&str>) -> Option<String> {
    let storage = if state.has_storage() {
        state.storage()
    } else {
        return None;
    };
    mount_signature_for_path(mountinfo?, &storage.root)
}

#[cfg(target_os = "linux")]
fn mount_signature_for_path(mountinfo: &str, path: &Path) -> Option<String> {
    let target = path.to_string_lossy();
    mountinfo
        .lines()
        .find(|line| line.split_whitespace().nth(4) == Some(target.as_ref()))
        .map(ToOwned::to_owned)
}

#[cfg(target_os = "linux")]
fn run_poll_loop(tx: tokio::sync::mpsc::Sender<()>) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let file = std::fs::File::open("/proc/self/mountinfo")?;
    let fd = file.as_raw_fd();

    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLPRI | libc::POLLERR,
        revents: 0,
    };

    loop {
        // SAFETY: pfd is a valid pollfd; nfds=1; timeout=-1 blocks forever.
        let rc = unsafe { libc::poll(&mut pfd, 1, -1) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if rc == 0 {
            continue;
        }
        if tx.blocking_send(()).is_err() {
            return Ok(());
        }
    }
}
