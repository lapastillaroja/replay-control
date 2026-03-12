use leptos::prelude::*;

use crate::i18n::{t, use_i18n};
use crate::server_fns;

/// A thin banner shown at the top of the page when a metadata operation
/// (import or thumbnail update) is running. Polls every ~3 seconds and
/// auto-hides when the operation finishes.
#[component]
pub fn MetadataBusyBanner() -> impl IntoView {
    let i18n = use_i18n();

    // A signal that ticks every ~3 seconds to trigger re-polling.
    let tick = RwSignal::new(0u32);

    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        Effect::new(move || {
            let window = match web_sys::window() {
                Some(w) => w,
                None => return,
            };
            let cb = Closure::<dyn Fn()>::new(move || {
                tick.update(|n| *n = n.wrapping_add(1));
            });
            let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                3000,
            );
            // The banner lives at the App root and never unmounts, so forget is fine.
            cb.forget();
        });
    }

    // LocalResource avoids the hydration mismatch warning: this is a
    // client-only runtime status check, not SSR-rendered content.
    let busy = LocalResource::new(move || {
        // Re-run whenever the tick signal changes (every ~3s on the client).
        let _ = tick.get();
        async move { server_fns::is_metadata_busy().await.unwrap_or(false) }
    });

    let is_busy = move || busy.get().map(|v| *v).unwrap_or(false);

    view! {
        <Show when=is_busy fallback=|| ()>
            <div class="metadata-busy-banner">
                <span class="metadata-busy-spinner"></span>
                {move || t(i18n.locale.get(), "metadata.busy_banner")}
            </div>
        </Show>
    }
}
