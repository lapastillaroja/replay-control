use leptos::prelude::*;

use crate::types::RomEntry;

#[component]
pub fn RomList(
    roms: Vec<RomEntry>,
    #[prop(into)] search_query: Signal<String>,
) -> impl IntoView {
    view! {
        <div class="rom-list">
            {roms
                .into_iter()
                .map(|rom| {
                    let filename = rom.filename.clone();
                    let filename_display = rom.filename.clone();
                    let filename_filter = rom.filename.clone();
                    let query = search_query;
                    let relative_path = rom.relative_path.clone();
                    let size = format_size(rom.size_bytes);
                    let ext = format!(
                        ".{}",
                        filename.rsplit('.').next().unwrap_or("")
                    );

                    view! {
                        <div
                            class="rom-item"
                            class:hidden=move || {
                                let q = query.get().to_lowercase();
                                !q.is_empty()
                                    && !filename_filter.to_lowercase().contains(&q)
                            }
                        >
                            <div class="rom-info">
                                <span class="rom-name">{filename_display}</span>
                                <span class="rom-path">{relative_path}</span>
                            </div>
                            <div class="rom-meta">
                                <span class="rom-size">{size}</span>
                                <span class="rom-ext">{ext}</span>
                            </div>
                        </div>
                    }
                })
                .collect::<Vec<_>>()}
        </div>
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
