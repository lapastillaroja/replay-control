use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::components::rom_list::RomList;
use crate::components::system_card::SystemCard;
use crate::i18n::{use_i18n, t};
use crate::server_fns;
use server_fn::ServerFnError;

/// `/games` — grid of all systems.
#[component]
pub fn GamesPage() -> impl IntoView {
    let i18n = use_i18n();
    let systems = Resource::new(|| (), |_| server_fns::get_systems());

    view! {
        <div class="page games-page">
            <h2 class="page-title">{move || t(i18n.locale.get(), "games.systems")}</h2>
            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let systems = systems.await?;
                        Ok::<_, ServerFnError>(view! {
                            <div class="systems-grid">
                                {systems.iter().map(|sys| {
                                    let href = format!("/games/{}", sys.folder_name);
                                    view! { <SystemCard system=sys.clone() href /> }
                                }).collect::<Vec<_>>()}
                            </div>
                        })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

/// `/games/:system` — ROM list for a specific system with infinite scroll.
#[component]
pub fn SystemRomView() -> impl IntoView {
    let params = use_params_map();
    let system = move || params.read().get("system").unwrap_or_default();

    view! {
        <div class="page games-page">
            <div class="system-rom-view">
                <RomList system=system() />
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
                    .map(|(_, e)| format!("{e}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            }}
        </div>
    }
}
