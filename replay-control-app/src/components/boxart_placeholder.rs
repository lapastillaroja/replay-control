use leptos::prelude::*;

/// CSS-only box-art placeholder for games without cover art.
///
/// Shows a system-colored background with the system abbreviation in large text
/// and the game name in smaller text below.
#[component]
pub fn BoxArtPlaceholder(
    /// System folder name (e.g., "nintendo_snes").
    system: String,
    /// Game display name or base title.
    name: String,
    /// Size variant: "list", "card", "hero", or "detail".
    #[prop(default = "card".to_string())]
    size: String,
) -> impl IntoView {
    let sys = replay_control_core::systems::find_system(&system);
    let abbrev = sys.map(|s| s.abbreviation).unwrap_or("?");
    let color = sys.map(|s| s.placeholder_color).unwrap_or("#333");
    let is_detail = size == "detail";
    let system_display = sys.map(|s| s.display_name).unwrap_or("").to_string();

    view! {
        <div
            class=format!("boxart-placeholder bp-{size}")
            style=format!("background-color: {color}; --bp-color: {color}")
        >
            <span class="bp-abbrev">{abbrev}</span>
            <span class="bp-name">{name}</span>
            {is_detail.then(|| view! {
                <span class="bp-system-display">{system_display}</span>
            })}
        </div>
    }
}
