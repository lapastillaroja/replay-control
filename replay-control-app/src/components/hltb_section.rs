use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, HltbData};

/// Shows HowLongToBeat completion times on the game detail page.
#[component]
pub fn HltbSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    base_title: StoredValue<String>,
    display_name: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    let hltb_data: RwSignal<Option<Option<HltbData>>> = RwSignal::new(None);
    let hltb_loading = RwSignal::new(false);
    let hltb_error = RwSignal::new(Option::<String>::None);

    let on_fetch_hltb = move |_| {
        let bt = base_title.get_value();
        let dn = display_name.get_value();
        hltb_loading.set(true);
        hltb_error.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::get_hltb_data(bt, dn).await {
                Ok(data) => hltb_data.set(Some(data)),
                Err(e) => hltb_error.set(Some(e.to_string())),
            }
            hltb_loading.set(false);
        });
    };

    // Suppress unused warnings — system/rom_filename kept for API consistency.
    let _ = system;
    let _ = rom_filename;

    view! {
        <section class="section hltb-section">
            <h2 class="section-title">{move || t(i18n.locale.get(), Key::HltbTitle)}</h2>

            {move || {
                match hltb_data.get() {
                    None => view! {
                        <div class="hltb-fetch-row">
                            <button
                                class="game-action-btn hltb-fetch-btn"
                                on:click=on_fetch_hltb
                                disabled=move || hltb_loading.get()
                            >
                                {move || {
                                    let locale = i18n.locale.get();
                                    if hltb_loading.get() {
                                        t(locale, Key::HltbFetching)
                                    } else {
                                        t(locale, Key::HltbFetch)
                                    }
                                }}
                            </button>
                            {move || hltb_error.get().map(|e| view! {
                                <p class="hltb-error">{e}</p>
                            })}
                        </div>
                    }
                    .into_any(),
                    Some(None) => view! {
                        <p class="hltb-no-data">
                            {move || t(i18n.locale.get(), Key::HltbNoData)}
                        </p>
                    }
                    .into_any(),
                    Some(Some(data)) => view! { <HltbTimes data /> }.into_any(),
                }
            }}
        </section>
    }
}

#[component]
fn HltbTimes(data: HltbData) -> impl IntoView {
    let i18n = use_i18n();
    let data = StoredValue::new(data);

    view! {
        <div class="hltb-times">
            {move || {
                data.get_value().main_secs.map(|s| {
                    view! {
                        <div class="hltb-time-card">
                            <span class="hltb-time-label">
                                {move || t(i18n.locale.get(), Key::HltbMain)}
                            </span>
                            <span class="hltb-time-value">{HltbData::format_hours(s)}</span>
                        </div>
                    }
                })
            }}
            {move || {
                data.get_value().main_extra_secs.map(|s| {
                    view! {
                        <div class="hltb-time-card">
                            <span class="hltb-time-label">
                                {move || t(i18n.locale.get(), Key::HltbMainExtra)}
                            </span>
                            <span class="hltb-time-value">{HltbData::format_hours(s)}</span>
                        </div>
                    }
                })
            }}
            {move || {
                data.get_value().completionist_secs.map(|s| {
                    view! {
                        <div class="hltb-time-card">
                            <span class="hltb-time-label">
                                {move || t(i18n.locale.get(), Key::HltbCompletionist)}
                            </span>
                            <span class="hltb-time-value">{HltbData::format_hours(s)}</span>
                        </div>
                    }
                })
            }}
        </div>
    }
}
