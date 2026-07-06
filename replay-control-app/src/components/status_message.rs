use leptos::prelude::*;

/// Inline ok/error status line shown after a save or action completes.
///
/// The state is `Some((ok, message))` once an operation finishes — `ok` picks
/// the success vs error styling — and `None` while idle. Shared by the settings
/// pages and the Net Control card, which previously each re-implemented this
/// same closure.
#[component]
pub fn StatusMessage(status: RwSignal<Option<(bool, String)>>) -> impl IntoView {
    move || {
        status.get().map(|(ok, msg)| {
            let class = if ok {
                "status-msg status-ok"
            } else {
                "status-msg status-err"
            };
            view! { <div class=class>{msg}</div> }
        })
    }
}
