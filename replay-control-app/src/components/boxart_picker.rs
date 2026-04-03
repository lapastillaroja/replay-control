use leptos::prelude::*;

use crate::i18n::{t, use_i18n, Key};
use crate::server_fns::{self, BoxArtVariant};

/// Bottom sheet for picking a box art variant.
///
/// Fetches variants on mount, shows a horizontal strip of thumbnails,
/// and calls `on_change` with the new image URL when the user picks one.
#[component]
pub fn BoxArtPicker(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    #[prop(into)] on_close: Callback<()>,
    #[prop(into)] on_change: Callback<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    let variants = RwSignal::new(Vec::<BoxArtVariant>::new());
    let loading = RwSignal::new(true);
    let applying = RwSignal::new(false);

    // Fetch variants on mount.
    let _fetch = Effect::new(move || {
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        leptos::task::spawn_local(async move {
            if let Ok(v) = server_fns::get_boxart_variants(sys, fname).await {
                variants.set(v);
            }
            loading.set(false);
        });
    });

    let on_select = move |variant_filename: String| {
        if applying.get() {
            return;
        }
        applying.set(true);
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        leptos::task::spawn_local(async move {
            match server_fns::set_boxart_override(sys, fname, variant_filename).await {
                Ok(new_url) => {
                    // Call on_change LAST — it unmounts this component (sets show_picker=false),
                    // which disposes all our signals. Don't touch `applying` after this.
                    on_change.run(new_url);
                }
                Err(_) => {
                    applying.set(false);
                }
            }
        });
    };

    let on_reset = move |_| {
        if applying.get() {
            return;
        }
        applying.set(true);
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        leptos::task::spawn_local(async move {
            match server_fns::reset_boxart_override(sys, fname).await {
                Ok(()) => {
                    // Same: on_change unmounts us, so call it last.
                    on_change.run(String::new());
                }
                Err(_) => {
                    applying.set(false);
                }
            }
        });
    };

    let on_overlay_click = move |_| {
        on_close.run(());
    };

    let on_sheet_click = move |ev: leptos::ev::MouseEvent| {
        // Prevent clicks on the sheet from closing it.
        ev.stop_propagation();
    };

    view! {
        <div class="boxart-picker-overlay" on:click=on_overlay_click>
            <div class="boxart-picker-sheet" on:click=on_sheet_click>
                <div class="boxart-picker-header">
                    <span class="boxart-picker-title">
                        {move || t(i18n.locale.get(), Key::GameDetailChooseBoxart)}
                    </span>
                    <button class="boxart-picker-close" on:click=move |ev: leptos::ev::MouseEvent| {
                        ev.stop_propagation();
                        on_close.run(());
                    }>
                        {"\u{2715}"}
                    </button>
                </div>

                <Show when=move || loading.get()>
                    <div class="boxart-picker-loading">
                        {move || t(i18n.locale.get(), Key::CommonLoading)}
                    </div>
                </Show>

                <Show when=move || !loading.get() && variants.read().is_empty()>
                    <div class="boxart-picker-empty">
                        {move || t(i18n.locale.get(), Key::GameDetailNoVariants)}
                    </div>
                </Show>

                <Show when=move || !loading.get() && !variants.read().is_empty()>
                    <div class="boxart-picker-strip">
                        {move || {
                            variants.get().into_iter().map(|v| {
                                let filename_for_click = v.filename.clone();
                                let is_active = v.is_active;
                                let image_url = v.image_url.clone().unwrap_or_default();
                                let has_image = !image_url.is_empty();
                                let region = if v.region_label.is_empty() {
                                    v.filename.clone()
                                } else {
                                    v.region_label.clone()
                                };
                                let region_alt = region.clone();

                                view! {
                                    <div
                                        class="boxart-variant"
                                        class:active=is_active
                                        on:click=move |_| {
                                            on_select(filename_for_click.clone());
                                        }
                                    >
                                        <Show when=move || has_image
                                            fallback=move || view! {
                                                <div class="boxart-variant-placeholder">
                                                    {"\u{2B07}"}
                                                </div>
                                            }
                                        >
                                            <img
                                                class="boxart-variant-img"
                                                src=image_url.clone()
                                                alt=region_alt.clone()
                                            />
                                        </Show>
                                        <span class="boxart-variant-label">{region}</span>
                                        <Show when=move || is_active>
                                            <span class="boxart-variant-check">{"\u{2713}"}</span>
                                        </Show>
                                    </div>
                                }
                            }).collect::<Vec<_>>()
                        }}
                    </div>

                    <button
                        class="boxart-picker-reset"
                        prop:disabled=move || applying.get()
                        on:click=on_reset
                    >
                        {move || {
                            if applying.get() {
                                t(i18n.locale.get(), Key::GameDetailDownloading)
                            } else {
                                t(i18n.locale.get(), Key::GameDetailResetDefault)
                            }
                        }}
                    </button>
                </Show>
            </div>
        </div>
    }
}
