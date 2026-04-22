// replay-control-core-server — native (linux) server-side implementation.
//
// Holds everything that touches rusqlite, deadpool-sqlite, tokio, reqwest,
// std::fs, std::process, or quick-xml. Pure types + wire contracts live in
// `replay-control-core` and are re-exported at each module level so consumers
// can reach `replay_control_core_server::<module>::<Type>` without bouncing
// through two crates.
//
// No top-level glob re-export of core: that would drag core's module names
// into this crate's namespace and collide with this crate's own `pub mod`
// declarations. See plan G2.

pub mod launch;
pub mod settings;

pub mod capture;
pub use capture::screenshots;
