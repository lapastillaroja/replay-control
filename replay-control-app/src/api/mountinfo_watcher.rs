//! Event-driven storage detection via `/proc/self/mountinfo`.
//!
//! The kernel signals mount-table changes by waking pollers waiting on
//! `POLLPRI` against `/proc/self/mountinfo` — the canonical event source
//! for "a mount appeared / went away". On non-Linux, [`spawn`] is a
//! no-op and the periodic poll in `spawn_storage_watcher` is the only
//! signal source.

use super::AppState;
use std::time::Duration;

/// Coalesce burst events from `mount` / remount sequences into a single
/// `refresh_storage` call.
const DEBOUNCE: Duration = Duration::from_millis(500);

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
        while rx.recv().await.is_some() {
            let deadline = tokio::time::Instant::now() + DEBOUNCE;
            loop {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(_)) => continue,
                    Ok(None) => return,
                    Err(_) => break,
                }
            }

            tracing::debug!("mountinfo: change detected, refreshing storage");
            match state.refresh_storage().await {
                Ok(true) => tracing::info!("Storage updated after mount-table change"),
                Ok(false) => tracing::debug!("mountinfo event but storage unchanged"),
                Err(e) => tracing::warn!("refresh_storage after mount event failed: {e}"),
            }
        }
    });
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
