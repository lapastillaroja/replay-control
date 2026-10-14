#[cfg(feature = "hydrate")]
use gloo_timers::callback::Timeout;
use leptos::prelude::*;

/// Inline ok/error status line shown after a save or action completes.
///
/// The state is `Some((ok, message))` once an operation finishes — `ok` picks
/// the success vs error styling — and `None` while idle. Shared by the settings
/// pages and the Net Control card, which previously each re-implemented this
/// same closure.
#[component]
pub fn StatusMessage(
    status: RwSignal<Option<(bool, String)>>,
    #[prop(optional)] dismiss_success_after_ms: Option<u32>,
) -> impl IntoView {
    #[cfg(not(feature = "hydrate"))]
    let _ = dismiss_success_after_ms;
    #[cfg(feature = "hydrate")]
    Effect::new(move |previous: Option<Option<Timeout>>| {
        drop(previous);
        match (dismiss_success_after_ms, status.get()) {
            (Some(delay), Some((true, _))) => Some(Timeout::new(delay, move || {
                status.try_set(None);
            })),
            _ => None,
        }
    });

    move || {
        status.get().map(|(ok, msg)| {
            let class = if ok {
                "status-msg status-ok"
            } else {
                "status-msg status-err"
            };
            view! { <div class=class role=if ok { "status" } else { "alert" }>{msg}</div> }
        })
    }
}
