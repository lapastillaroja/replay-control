use leptos::prelude::*;

use crate::i18n::{Key, t};

/// One selectable system chip: the system id (the filter value), its display
/// name, and the game count shown in the label.
#[derive(Clone)]
pub struct FilterChipSystem {
    pub system: String,
    pub display: String,
    pub count: usize,
}

/// Horizontal system-filter chips shared by the developer and board facet
/// pages: an "All (N)" chip plus one chip per system, toggling `system_filter`.
/// Renders nothing when there's a single system — there's nothing to filter.
#[component]
pub fn SystemFilterChips(
    systems: Vec<FilterChipSystem>,
    system_filter: RwSignal<String>,
    locale: crate::i18n::Locale,
) -> impl IntoView {
    if systems.len() <= 1 {
        return view! { <div /> }.into_any();
    }

    let total_count: usize = systems.iter().map(|s| s.count).sum();
    let all_label = format!("{} ({})", t(locale, Key::DeveloperAllSystems), total_count);

    view! {
        <div class="system-filter-chips">
            <button
                class=move || if system_filter.read().is_empty() {
                    "system-chip system-chip-active"
                } else {
                    "system-chip"
                }
                on:click=move |_| system_filter.set(String::new())
            >
                {all_label}
            </button>
            {systems.into_iter().map(|sys| {
                let sys_id = sys.system.clone();
                let label = format!("{} ({})", sys.display, sys.count);
                let sys_for_check = sys_id.clone();
                view! {
                    <button
                        class=move || if *system_filter.read() == sys_for_check {
                            "system-chip system-chip-active"
                        } else {
                            "system-chip"
                        }
                        on:click=move |_| system_filter.set(sys_id.clone())
                    >
                        {label}
                    </button>
                }
            }).collect::<Vec<_>>()}
        </div>
    }
    .into_any()
}
