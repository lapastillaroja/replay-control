use std::io::Write;

/// Append a debug message to /tmp/replay-core-debug.log.
/// Silently ignores errors (logging must never crash the core).
pub fn debug_log(msg: &str) {
    let _ = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/replay-core-debug.log")?;
        writeln!(f, "{}", msg)?;
        Ok(())
    })();
}
