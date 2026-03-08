use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::components::rom_list::RomList;
use crate::components::system_card::SystemCard;
use crate::i18n::{use_i18n, t};
use crate::server_fns;

/// `/games` — grid of all systems.
#[component]
pub fn GamesPage() -> impl IntoView {
    let i18n = use_i18n();
    let systems = Resource::new(|| (), |_| server_fns::get_systems());

    view! {
        <div class="page games-page">
            <h2 class="page-title">{move || t(i18n.locale.get(), "games.systems")}</h2>
            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                {move || Suspend::new(async move {
                    let locale = i18n.locale.get();
                    match systems.await {
                        Ok(systems) => {
                            view! {
                                <div class="systems-grid">
                                    {systems.iter().map(|sys| {
                                        let href = format!("/games/{}", sys.folder_name);
                                        view! { <SystemCard system=sys.clone() href /> }
                                    }).collect::<Vec<_>>()}
                                </div>
                            }.into_any()
                        }
                        Err(e) => {
                            view! { <p class="error">{format!("{}: {e}", t(locale, "common.error"))}</p> }.into_any()
                        }
                    }
                })}
            </Suspense>
        </div>
    }
}

/// `/games/:system` — ROM list for a specific system with infinite scroll.
#[component]
pub fn SystemRomView() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let system = move || params.read().get("system").unwrap_or_default();

    view! {
        <div class="page games-page">
            <div class="system-rom-view">
                <div class="rom-header">
                    <A href="/games" attr:class="back-btn">
                        {move || t(i18n.locale.get(), "games.back")}
                    </A>
                    <h2 class="page-title">{system}</h2>
                </div>
                <RomList system=system() />
            </div>
        </div>
    }
}
