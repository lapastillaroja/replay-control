use leptos::prelude::*;
use leptos_router::components::A;

use replay_control_core::update::UpdateState;

use crate::hooks::use_update_state;
use crate::i18n::{Key, t, use_i18n};

/// Compact update notice shown at the top of the page content on every screen
/// when an update is available. It sits in normal flow (scrolls away with the
/// page rather than pinning) since an available update is informational, not
/// urgent. It links into Settings → Updates, where the full "what's new"
/// changelog and the Update Now / Skip actions live.
#[component]
pub fn UpdateAvailableBanner() -> impl IntoView {
    let i18n = use_i18n();
    let update_state = use_update_state();

    let version = move || match update_state.get() {
        UpdateState::Available(available) => Some(available.version),
        _ => None,
    };

    view! {
        <Show when=move || version().is_some() fallback=|| ()>
            <A href="/settings#settings-updates" attr:class="update-notice-banner">
                <span class="update-notice-banner-text">
                    {move || {
                        t(i18n.locale.get(), Key::UpdateAvailable)
                            .replace("{0}", &version().unwrap_or_default())
                    }}
                </span>
                <span class="update-notice-banner-cta" aria-hidden="true">
                    "›"
                </span>
            </A>
        </Show>
    }
}
