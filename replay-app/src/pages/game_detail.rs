use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use server_fn::ServerFnError;

use crate::i18n::{use_i18n, t};
use crate::pages::ErrorDisplay;
use crate::server_fns::{self, ArcadeMetadata, RomDetail};
use crate::util::format_size;

#[component]
pub fn GameDetailPage() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let system = move || params.read().get("system").unwrap_or_default();
    let filename = move || {
        let raw = params.read().get("filename").unwrap_or_default();
        // URL-decode the filename
        urlencoding::decode(&raw)
            .map(|s| s.into_owned())
            .unwrap_or(raw)
    };

    let detail = Resource::new(
        move || (system(), filename()),
        |(sys, fname)| server_fns::get_rom_detail(sys, fname),
    );

    view! {
        <div class="page game-detail">
            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let data = detail.await?;
                        Ok::<_, ServerFnError>(view! {
                            <GameDetailContent detail=data system=system() />
                        })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

#[component]
fn GameDetailContent(detail: RomDetail, system: String) -> impl IntoView {
    let i18n = use_i18n();

    let game_name = detail
        .rom
        .game
        .display_name
        .clone()
        .unwrap_or_else(|| detail.rom.game.rom_filename.clone());
    let game_name_sv = StoredValue::new(game_name.clone());
    let filename_sv = StoredValue::new(detail.rom.game.rom_filename.clone());
    let relative_path_sv = StoredValue::new(detail.rom.game.rom_path.clone());
    let system_sv = StoredValue::new(system.clone());
    let system_display = detail.rom.game.system_display.clone();
    let size_display = format_size(detail.rom.size_bytes);
    let ext = detail
        .rom
        .game
        .rom_filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_uppercase();
    let back_href = format!("/games/{system}");

    let is_favorite = RwSignal::new(detail.is_favorite);
    let arcade_info = detail.arcade_info.clone();
    let has_arcade = arcade_info.is_some();

    // Delete confirmation state
    let confirming_delete = RwSignal::new(false);

    // Rename state
    let is_renaming = RwSignal::new(false);
    let rename_value = RwSignal::new(detail.rom.game.rom_filename.clone());

    // Toggle favorite
    let on_toggle_fav = move |_| {
        let fav = is_favorite.get();
        is_favorite.set(!fav);

        let sys = system_sv.get_value();
        let fname = filename_sv.get_value();
        let rp = relative_path_sv.get_value();

        if fav {
            let fav_filename = format!("{sys}@{fname}.fav");
            leptos::task::spawn_local(async move {
                let _ = server_fns::remove_favorite(fav_filename, None).await;
            });
        } else {
            leptos::task::spawn_local(async move {
                let _ = server_fns::add_favorite(sys, rp, false).await;
            });
        }
    };

    let fav_label = move || {
        let locale = i18n.locale.get();
        if is_favorite.get() {
            t(locale, "game_detail.unfavorite")
        } else {
            t(locale, "game_detail.favorite")
        }
    };

    let fav_icon = move || {
        if is_favorite.get() { "\u{2605}" } else { "\u{2606}" }
    };

    view! {
        // Header
        <div class="rom-header">
            <A href=back_href attr:class="back-btn">
                {move || t(i18n.locale.get(), "games.back")}
            </A>
            <h2 class="page-title">{game_name.clone()}</h2>
        </div>

        // Hero / Cover Art
        <section class="section">
            <div class="game-cover">
                <span class="game-cover-text">{game_name_sv.get_value()}</span>
            </div>
        </section>

        // Game Info Card
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), "game_detail.info")}</h2>
            <div class="game-meta-grid">
                <div class="game-meta-item">
                    <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.system")}</span>
                    <span class="game-meta-value">{system_display.clone()}</span>
                </div>
                <div class="game-meta-item">
                    <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.filename")}</span>
                    <span class="game-meta-value">{filename_sv.get_value()}</span>
                </div>
                <div class="game-meta-item">
                    <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.file_size")}</span>
                    <span class="game-meta-value">{size_display}</span>
                </div>
                <Show when=move || !has_arcade>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.format")}</span>
                        <span class="game-meta-value">{ext.clone()}</span>
                    </div>
                </Show>
            </div>
        </section>

        // Arcade Info (if applicable)
        {arcade_info.map(|info| view! { <ArcadeInfoSection info /> })}

        // Description
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.description")}</h2>
            <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_description")}</p>
        </section>

        // Screenshots Gallery
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.screenshots")}</h2>
            <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_screenshots")}</p>
        </section>

        // Videos
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.videos")}</h2>
            <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_videos")}</p>
        </section>

        // Music / Soundtrack
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.music")}</h2>
            <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_music")}</p>
        </section>

        // Manual
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.manual")}</h2>
            <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_manual")}</p>
        </section>

        // Actions
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), "game_detail.actions")}</h2>
            <div class="game-actions">
                <button class="game-action-btn game-action-fav" on:click=on_toggle_fav>
                    <span class="game-action-icon">{fav_icon}</span>
                    {fav_label}
                </button>

                <GameRenameAction
                    is_renaming rename_value
                    filename=filename_sv
                    relative_path=relative_path_sv
                    system=system_sv
                />

                <GameDeleteAction
                    confirming_delete
                    relative_path=relative_path_sv
                    system=system_sv
                />
            </div>
        </section>
    }
}

/// Rename action: shows a button that toggles to an inline rename form.
#[component]
fn GameRenameAction(
    is_renaming: RwSignal<bool>,
    rename_value: RwSignal<String>,
    filename: StoredValue<String>,
    relative_path: StoredValue<String>,
    system: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let navigate = use_navigate();

    let do_rename = StoredValue::new(move || {
        let rp = relative_path.get_value();
        let new_name = rename_value.get();
        let sys = system.get_value();
        is_renaming.set(false);
        let nav = navigate.clone();
        leptos::task::spawn_local(async move {
            if server_fns::rename_rom(rp, new_name.clone()).await.is_ok() {
                let encoded = urlencoding::encode(&new_name);
                let href = format!("/games/{sys}/{encoded}");
                nav(&href, Default::default());
            }
        });
    });

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" {
            (do_rename.get_value())();
        } else if ev.key() == "Escape" {
            is_renaming.set(false);
        }
    };

    let on_click = move |_: leptos::ev::MouseEvent| {
        (do_rename.get_value())();
    };

    view! {
        <Show when=move || is_renaming.get() fallback=move || view! {
            <button class="game-action-btn" on:click=move |_| {
                rename_value.set(filename.get_value());
                is_renaming.set(true);
            }>
                <span class="game-action-icon">{"\u{270F}"}</span>
                {move || t(i18n.locale.get(), "game_detail.rename")}
            </button>
        }>
            <div class="game-rename-inline">
                <input
                    type="text"
                    class="rename-input"
                    prop:value=move || rename_value.get()
                    on:input=move |ev| rename_value.set(event_target_value(&ev))
                    on:keydown=on_keydown
                />
                <div class="game-rename-btns">
                    <button class="rom-action-btn" on:click=on_click>
                        {"\u{2713}"}
                    </button>
                    <button class="rom-action-btn" on:click=move |_| is_renaming.set(false)>
                        {"\u{2715}"}
                    </button>
                </div>
            </div>
        </Show>
    }
}

/// Delete action: shows a button that toggles to a confirm/cancel pair.
#[component]
fn GameDeleteAction(
    confirming_delete: RwSignal<bool>,
    relative_path: StoredValue<String>,
    system: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let navigate = use_navigate();

    let nav_sv = StoredValue::new(navigate);
    let on_delete = move |_| {
        let rp = relative_path.get_value();
        let sys = system.get_value();
        let nav = nav_sv.get_value();
        leptos::task::spawn_local(async move {
            if server_fns::delete_rom(rp).await.is_ok() {
                let href = format!("/games/{sys}");
                nav(&href, Default::default());
            }
        });
    };

    view! {
        <Show when=move || confirming_delete.get() fallback=move || view! {
            <button class="game-action-btn game-action-delete" on:click=move |_| confirming_delete.set(true)>
                <span class="game-action-icon">{"\u{2715}"}</span>
                {move || t(i18n.locale.get(), "game_detail.delete")}
            </button>
        }>
            <div class="game-delete-confirm">
                <button class="game-action-btn game-action-delete-confirm" on:click=on_delete>
                    {move || t(i18n.locale.get(), "game_detail.confirm_delete")}
                </button>
                <button class="game-action-btn" on:click=move |_| confirming_delete.set(false)>
                    {move || t(i18n.locale.get(), "games.cancel")}
                </button>
            </div>
        </Show>
    }
}

#[component]
fn ArcadeInfoSection(info: ArcadeMetadata) -> impl IntoView {
    let i18n = use_i18n();

    let year = StoredValue::new(info.year);
    let manufacturer = StoredValue::new(info.manufacturer);
    let players = info.players;
    let rotation = info.rotation;
    let category = StoredValue::new(info.category);
    let is_clone = info.is_clone;
    let parent = info.parent;

    let has_year = !year.get_value().is_empty();
    let has_manufacturer = !manufacturer.get_value().is_empty();
    let has_players = players != 0;
    let has_category = !category.get_value().is_empty();

    view! {
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), "game_detail.arcade_info")}</h2>
            <div class="game-meta-grid">
                <Show when=move || has_year>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.year")}</span>
                        <span class="game-meta-value">{year.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_manufacturer>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.manufacturer")}</span>
                        <span class="game-meta-value">{manufacturer.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_players>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.players")}</span>
                        <span class="game-meta-value">{players.to_string()}</span>
                    </div>
                </Show>
                <div class="game-meta-item">
                    <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.rotation")}</span>
                    <span class="game-meta-value">{rotation.clone()}</span>
                </div>
                <Show when=move || has_category>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.category")}</span>
                        <span class="game-meta-value">{category.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || is_clone>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.parent_rom")}</span>
                        <span class="game-meta-value">{parent.clone()}</span>
                    </div>
                </Show>
            </div>
        </section>
    }
}
