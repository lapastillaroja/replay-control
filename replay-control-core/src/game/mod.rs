pub mod arcade_db;
pub mod date_precision;
pub mod developer;
pub mod game_db;
pub mod game_ref;
pub mod genre;
pub mod rom_tags;
pub mod series_db;
pub mod title_utils;

/// Whether this test binary was built against the committed `fixtures/` stubs
/// rather than the real `data/` sources. The value of `REPLAY_BUILD_STUB` is
/// captured at compile time via `option_env!`, so toggling the flag triggers
/// a rebuild (see `cargo::rerun-if-env-changed` in `build.rs`).
#[cfg(test)]
pub(crate) fn using_stub_data() -> bool {
    matches!(option_env!("REPLAY_BUILD_STUB"), Some("1") | Some("true"))
}
