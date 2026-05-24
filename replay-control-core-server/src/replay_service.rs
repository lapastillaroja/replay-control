use std::path::Path;

use replay_control_core::error::{Error, Result};

const REPLAYOS_MARKER: &str = "/opt/replay";
const REPLAY_SERVICE: &str = "replay.service";

pub fn is_replayos() -> bool {
    Path::new(REPLAYOS_MARKER).exists()
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

pub async fn restart_async() -> Result<()> {
    let output = tokio::process::Command::new("systemctl")
        .args(["restart", REPLAY_SERVICE])
        .output()
        .await
        .map_err(|e| Error::io(Path::new("systemctl"), e))?;

    status_to_result("restart", output.status.success(), &output.stderr)
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
