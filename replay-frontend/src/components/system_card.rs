use leptos::prelude::*;

use crate::types::SystemSummary;

#[component]
pub fn SystemCard(
    system: SystemSummary,
    on_click: impl Fn(String) + 'static,
) -> impl IntoView {
    let has_games = system.game_count > 0;
    let folder = system.folder_name.clone();
    let size_display = format_size(system.total_size_bytes);
    let display_name = system.display_name.clone();
    let manufacturer = system.manufacturer.clone();
    let count_text = if has_games {
        format!("{} games", system.game_count)
    } else {
        "No games".to_string()
    };

    view! {
        <button
            class="system-card"
            class:empty=!has_games
            on:click=move |_| on_click(folder.clone())
        >
            <div class="system-card-name">{display_name}</div>
            <div class="system-card-manufacturer">{manufacturer}</div>
            <div class="system-card-count">{count_text}</div>
            <Show when=move || has_games>
                <div class="system-card-size">{size_display.clone()}</div>
            </Show>
        </button>
    }
}

fn format_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{} KB", bytes / 1024)
    }
}
