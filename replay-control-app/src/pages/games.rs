use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::components::rom_list::RomList;

/// `/games/:system` — ROM list for a specific system with infinite scroll.
#[component]
pub fn SystemRomView() -> impl IntoView {
    let params = use_params_map();
    let system = params.read_untracked().get("system").unwrap_or_default();

    view! {
        <div class="page games-page">
            <div class="system-rom-view">
                <RomList system />
            </div>
        </div>
    }
}

/// Shared error display for ErrorBoundary fallbacks.
#[component]
pub fn ErrorDisplay(errors: ArcRwSignal<Errors>) -> impl IntoView {
    view! {
        <div class="error">
            {move || {
                errors.read()
                    .iter()
                    .map(|(_, e)| {
                        let msg = format!("{e}");
                        // Strip Leptos's "error running server function: " prefix
                        // for a cleaner user-facing message.
                        msg.strip_prefix("error running server function: ")
                            .unwrap_or(&msg)
                            .to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            }}
        </div>
    }
}
