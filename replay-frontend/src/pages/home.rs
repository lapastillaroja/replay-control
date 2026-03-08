use leptos::prelude::*;

use crate::api;
use crate::Tab;

#[component]
pub fn HomePage(
    set_selected_system: WriteSignal<Option<String>>,
    set_active_tab: WriteSignal<Tab>,
) -> impl IntoView {
    let info = LocalResource::new(|| api::fetch_info());
    let recents = LocalResource::new(|| api::fetch_recents());
    let systems = LocalResource::new(|| api::fetch_systems());

    view! {
        <div class="page home-page">
            <section class="section">
                <h2 class="section-title">"Last Played"</h2>
                <Suspense fallback=|| view! { <div class="loading">"Loading..."</div> }>
                    {move || {
                        recents
                            .get()
                            .map(|result| {
                                match &*result {
                                    Ok(entries) => {
                                        if let Some(last) = entries.first() {
                                            let name = last.rom_filename.clone();
                                            let sys = last.system_display.clone();
                                            view! {
                                                <div class="hero-card">
                                                    <div class="hero-info">
                                                        <h3 class="hero-title">{name}</h3>
                                                        <p class="hero-system">{sys}</p>
                                                    </div>
                                                </div>
                                            }
                                                .into_any()
                                        } else {
                                            view! {
                                                <p class="empty-state">"No games played yet"</p>
                                            }
                                                .into_any()
                                        }
                                    }
                                    Err(e) => {
                                        view! { <p class="error">{format!("Error: {e}")}</p> }
                                            .into_any()
                                    }
                                }
                            })
                    }}
                </Suspense>
            </section>

            <section class="section">
                <h2 class="section-title">"Recently Played"</h2>
                <Suspense fallback=|| view! { <div class="loading">"Loading..."</div> }>
                    {move || {
                        recents
                            .get()
                            .map(|result| {
                                match &*result {
                                    Ok(entries) => {
                                        let items: Vec<_> =
                                            entries.iter().skip(1).take(10).cloned().collect();
                                        if items.is_empty() {
                                            view! {
                                                <p class="empty-state">"No recent games"</p>
                                            }
                                                .into_any()
                                        } else {
                                            view! {
                                                <div class="recent-scroll">
                                                    {items
                                                        .into_iter()
                                                        .map(|entry| {
                                                            let name = entry.rom_filename.clone();
                                                            let sys = entry.system_display.clone();
                                                            view! {
                                                                <div class="recent-item">
                                                                    <div class="recent-name">{name}</div>
                                                                    <div class="recent-system">{sys}</div>
                                                                </div>
                                                            }
                                                        })
                                                        .collect::<Vec<_>>()}
                                                </div>
                                            }
                                                .into_any()
                                        }
                                    }
                                    Err(e) => {
                                        view! { <p class="error">{format!("Error: {e}")}</p> }
                                            .into_any()
                                    }
                                }
                            })
                    }}
                </Suspense>
            </section>

            <section class="section">
                <h2 class="section-title">"Library"</h2>
                <Suspense fallback=|| view! { <div class="loading">"Loading..."</div> }>
                    {move || {
                        info.get()
                            .map(|result| {
                                match &*result {
                                    Ok(info) => {
                                        let games = info.total_games.to_string();
                                        let sys_count = info.systems_with_games.to_string();
                                        let favs = info.total_favorites.to_string();
                                        let used = format_size(info.disk_used_bytes);
                                        view! {
                                            <div class="stats-grid">
                                                <div class="stat-card">
                                                    <div class="stat-value">{games}</div>
                                                    <div class="stat-label">"Games"</div>
                                                </div>
                                                <div class="stat-card">
                                                    <div class="stat-value">{sys_count}</div>
                                                    <div class="stat-label">"Systems"</div>
                                                </div>
                                                <div class="stat-card">
                                                    <div class="stat-value">{favs}</div>
                                                    <div class="stat-label">"Favorites"</div>
                                                </div>
                                                <div class="stat-card">
                                                    <div class="stat-value">{used}</div>
                                                    <div class="stat-label">"Used"</div>
                                                </div>
                                            </div>
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
            </section>

            <section class="section">
                <h2 class="section-title">"Systems"</h2>
                <Suspense fallback=|| view! { <div class="loading">"Loading..."</div> }>
                    {move || {
                        systems
                            .get()
                            .map(|result| {
                                match &*result {
                                    Ok(systems) => {
                                        let with_games: Vec<_> = systems
                                            .iter()
                                            .filter(|s| s.game_count > 0)
                                            .cloned()
                                            .collect();
                                        view! {
                                            <div class="systems-grid">
                                                {with_games
                                                    .into_iter()
                                                    .map(|sys| {
                                                        let folder = sys.folder_name.clone();
                                                        let name = sys.display_name.clone();
                                                        let count =
                                                            format!("{} games", sys.game_count);
                                                        view! {
                                                            <button
                                                                class="system-card"
                                                                on:click=move |_| {
                                                                    set_selected_system
                                                                        .set(Some(folder.clone()));
                                                                    set_active_tab.set(Tab::Games);
                                                                }
                                                            >
                                                                <div class="system-card-name">
                                                                    {name}
                                                                </div>
                                                                <div class="system-card-count">
                                                                    {count}
                                                                </div>
                                                            </button>
                                                        }
                                                    })
                                                    .collect::<Vec<_>>()}
                                            </div>
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
            </section>
        </div>
    }
}

fn format_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}
