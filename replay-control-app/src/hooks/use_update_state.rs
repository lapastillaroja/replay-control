use leptos::prelude::*;

use replay_control_core::update::UpdateState;

/// Access the app-level update-lifecycle signal provided at the root.
///
/// The `RwSignal<UpdateState>` is provided once in `App` (see `lib.rs`). This
/// accessor centralizes the `use_context` + fallback so consumers don't each
/// re-derive it; the fallback keeps components renderable in isolation (e.g.
/// unit-rendered without the provider) by yielding a detached `None` signal.
pub fn use_update_state() -> RwSignal<UpdateState> {
    use_context::<RwSignal<UpdateState>>().unwrap_or_else(|| RwSignal::new(UpdateState::None))
}
