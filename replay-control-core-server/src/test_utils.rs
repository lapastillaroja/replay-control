//! Shared fixtures for unit and integration tests.
//!
//! Workspace-internal crate, so these helpers are exposed unconditionally as a
//! plain `pub mod test_utils` — `cargo test` picks them up with no feature
//! flag, and unused symbols are dropped from release binaries by LTO. Switch
//! to a feature-gated module if this crate is ever published.

#[cfg(feature = "library")]
#[cfg(feature = "library")]
use tempfile::TempDir;

#[cfg(feature = "library")]
use crate::DbPool;
#[cfg(feature = "library")]
use crate::library_db::LibraryDb;

/// Build a real `DbPool` over a fresh library DB inside a tempdir.
///
/// Schema is initialised by `LibraryDb::open`, the same path used in
/// production. Drop the returned `TempDir` last; the pool retains a path
/// reference to it.
#[cfg(feature = "library")]
pub fn build_library_pool() -> (DbPool, TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db_path = tmp.path().join("library.db");
    LibraryDb::open_at(&db_path).expect("open library db");
    let pool = DbPool::new(db_path, "library_db", LibraryDb::open_at, 1).expect("build pool");
    (pool, tmp)
}

/// Spin up a minimal HTTP server that answers every request with the given
/// status line + JSON body until the returned guard is dropped. Returns the
/// RePlayOS-style base URL (`http://127.0.0.1:<port>/api/v1`).
///
/// For exercising `replay_api::ReplayApiClient` and the status machines built
/// on it without a real RePlayOS frontend.
pub fn mock_replay_api(status_line: &'static str, body: &'static str) -> String {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock replay api");
    let addr = listener.local_addr().expect("mock replay api addr");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://{addr}/api/v1")
}

/// A 127.0.0.1 base URL with nothing listening — connection refused.
pub fn refused_replay_api() -> String {
    use std::net::TcpListener;

    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind probe");
        listener.local_addr().expect("probe addr").port()
    };
    format!("http://127.0.0.1:{port}/api/v1")
}
