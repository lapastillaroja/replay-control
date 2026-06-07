use std::path::{Path, PathBuf};

use replay_control_core::error::{Error, Result};
use replay_control_core::runtime_env::Mode;

const REPLAYOS_MARKER: &str = "/opt/replay";
const REPLAY_SERVICE: &str = "replay.service";

pub fn is_replayos() -> bool {
    Path::new(REPLAYOS_MARKER).exists()
}

/// Detect the deployment mode once at startup. `--storage-path` is the
/// Standalone trigger (a path was explicitly given, so don't auto-detect); no
/// override on RePlayOS hardware means Device mode; neither is an error (the
/// app can't run off-device without a ROM library to point at).
///
/// The storage path is moved into `Mode::Standalone`, so callers never need to
/// remember a parallel `Option<PathBuf>` — the variant itself is the answer to
/// "where does `replay.cfg` live?".
pub fn detect_mode(storage_path: Option<PathBuf>) -> Result<Mode> {
    match (storage_path, is_replayos()) {
        (Some(storage_root), _) => Ok(Mode::Standalone { storage_root }),
        (None, true) => Ok(Mode::Device),
        (None, false) => Err(Error::Other(
            "Replay Control needs either the RePlayOS device or a --storage-path \
             pointing at a ROM library."
                .to_string(),
        )),
    }
}

pub fn start() -> Result<()> {
    systemctl("start")
}

pub fn stop() -> Result<()> {
    systemctl("stop")
}

pub fn restart() -> Result<()> {
    systemctl("restart")
}

fn systemctl(action: &str) -> Result<()> {
    let output = std::process::Command::new("systemctl")
        .args([action, REPLAY_SERVICE])
        .output()
        .map_err(|e| Error::io(Path::new("systemctl"), e))?;

    status_to_result(action, output.status.success(), &output.stderr)
}

fn status_to_result(action: &str, success: bool, stderr: &[u8]) -> Result<()> {
    if success {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(stderr).trim().to_string();
        Err(Error::Other(format!(
            "Failed to {action} {REPLAY_SERVICE}: {stderr}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_mode_with_storage_override_is_standalone() {
        // The Standalone trigger is `--storage-path` presence — independent of
        // whether the marker is present (e.g. dev on the actual device). The
        // path is moved into the Mode variant so callers never need a parallel
        // Option<PathBuf>.
        let m = detect_mode(Some(PathBuf::from("/tmp/anywhere"))).unwrap();
        assert_eq!(m.standalone_root(), Some(Path::new("/tmp/anywhere")));
        assert!(!m.is_device());
    }

    #[test]
    fn detect_mode_off_device_without_override_errors() {
        // The test runner is not RePlayOS (no `/opt/replay`), so no override
        // means we can't pick a mode.
        if is_replayos() {
            return; // skip on the unlikely chance tests run on the device
        }
        assert!(detect_mode(None).is_err());
    }
}
