use leptos::prelude::*;

/// Small card displaying a single stat value and label.
///
/// Used on the home page (library stats) and the metadata page
/// (library summary cards). Pass `compact=true` for a denser layout.
#[component]
pub fn StatCard(
    #[prop(into)] value: String,
    label: &'static str,
    #[prop(optional)] compact: bool,
) -> impl IntoView {
    let class = if compact {
        "stat-card compact"
    } else {
        "stat-card"
    };
    view! {
        <div class=class>
            <div class="stat-value">{value}</div>
            <div class="stat-label">{label}</div>
        </div>
    }
}
