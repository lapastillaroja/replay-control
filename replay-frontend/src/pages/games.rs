use leptos::prelude::*;

use crate::api;
use crate::components::rom_list::RomList;
use crate::components::system_card::SystemCard;

#[component]
pub fn GamesPage(
    selected_system: ReadSignal<Option<String>>,
    set_selected_system: WriteSignal<Option<String>>,
) -> impl IntoView {
    let systems = LocalResource::new(|| api::fetch_systems());

    view! {
        <div class="page games-page">
            <Show
                when=move || selected_system.get().is_some()
                fallback=move || {
                    view! {
                        <h2 class="page-title">"Systems"</h2>
                        <Suspense fallback=|| view! { <div class="loading">"Loading..."</div> }>
                            {move || {
                                systems
                                    .get()
                                    .map(|result| {
                                        match &*result {
                                            Ok(systems) => {
                                                view! {
                                                    <div class="systems-grid">
                                                        {systems
                                                            .iter()
                                                            .map(|sys| {
                                                                let set_sys = set_selected_system;
                                                                view! {
                                                                    <SystemCard
                                                                        system=sys.clone()
                                                                        on_click=move |f: String| {
                                                                            set_sys.set(Some(f))
                                                                        }
                                                                    />
                                                                }
                                                            })
                                                            .collect::<Vec<_>>()}
                                                    </div>
                                                }
                                                    .into_any()
                                            }
                                            Err(e) => {
                                                view! {
                                                    <p class="error">{format!("Error: {e}")}</p>
                                                }
                                                    .into_any()
                                            }
                                        }
                                    })
                            }}
                        </Suspense>
                    }
                }
            >
                {move || {
                    selected_system
                        .get()
                        .map(|system| {
                            view! { <SystemRomView system=system set_selected_system /> }
                        })
                }}
            </Show>
        </div>
    }
}

#[component]
fn SystemRomView(
    system: String,
    set_selected_system: WriteSignal<Option<String>>,
) -> impl IntoView {
    let system_clone = system.clone();
    let system_title = system.clone();
    let roms = LocalResource::new(move || {
        let s = system_clone.clone();
        async move { api::fetch_roms(&s).await }
    });

    let (search, set_search) = signal(String::new());

    view! {
        <div class="system-rom-view">
            <div class="rom-header">
                <button class="back-btn" on:click=move |_| set_selected_system.set(None)>
                    "\u{2190} Back"
                </button>
                <h2 class="page-title">{system_title}</h2>
            </div>
            <div class="search-bar">
                <input
                    type="text"
                    placeholder="Search games..."
                    class="search-input"
                    on:input=move |ev| {
                        set_search.set(event_target_value(&ev));
                    }
                />
            </div>
            <Suspense fallback=|| view! { <div class="loading">"Loading ROMs..."</div> }>
                {move || {
                    roms.get()
                        .map(|result| {
                            match &*result {
                                Ok(rom_list) => {
                                    let count = rom_list.len();
                                    view! {
                                        <p class="rom-count">{format!("{count} games")}</p>
                                        <RomList roms=rom_list.clone() search_query=search />
                                    }
                                        .into_any()
                                }
                                Err(e) => {
                                    view! { <p class="error">{format!("Error: {e}")}</p> }
                                        .into_any()
                                }
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}
