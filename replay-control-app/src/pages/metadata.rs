use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{use_i18n, t};
use crate::pages::ErrorDisplay;
use crate::server_fns;
use crate::util::format_size;

#[component]
pub fn MetadataPage() -> impl IntoView {
    let i18n = use_i18n();
    let stats = Resource::new(|| (), |_| server_fns::get_metadata_stats());

    // Import state
    let xml_path = RwSignal::new(String::new());
    let importing = RwSignal::new(false);
    let import_result = RwSignal::new(Option::<String>::None);

    // Clear state
    let confirming_clear = RwSignal::new(false);
    let clearing = RwSignal::new(false);
    let clear_result = RwSignal::new(Option::<String>::None);

    let on_import = move |_| {
        let path = xml_path.get();
        if path.is_empty() {
            return;
        }
        importing.set(true);
        import_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::import_launchbox_metadata(path).await {
                Ok(result) => {
                    import_result.set(Some(format!(
                        "{}: {} {}, {} {}",
                        "Import complete",
                        result.matched,
                        "matched",
                        result.inserted,
                        "inserted",
                    )));
                    stats.refetch();
                }
                Err(e) => {
                    import_result.set(Some(format!("Error: {e}")));
                }
            }
            importing.set(false);
        });
    };

    let on_clear = move |_| {
        clearing.set(true);
        clear_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::clear_metadata().await {
                Ok(()) => {
                    clear_result.set(Some("Metadata cleared".to_string()));
                    stats.refetch();
                }
                Err(e) => {
                    clear_result.set(Some(format!("Error: {e}")));
                }
            }
            clearing.set(false);
            confirming_clear.set(false);
        });
    };

    view! {
        <div class="page metadata-page">
            <div class="rom-header">
                <A href="/more" attr:class="back-btn">
                    {move || t(i18n.locale.get(), "games.back")}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), "metadata.title")}</h2>
            </div>

            // Status section
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.status")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let data = stats.await?;
                            Ok::<_, ServerFnError>(if data.total_entries == 0 {
                                view! {
                                    <p class="game-section-empty">{t(locale, "metadata.no_data")}</p>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="info-grid">
                                        <div class="info-row">
                                            <span class="info-label">{t(locale, "metadata.total_entries")}</span>
                                            <span class="info-value">{data.total_entries.to_string()}</span>
                                        </div>
                                        <div class="info-row">
                                            <span class="info-label">{t(locale, "metadata.with_description")}</span>
                                            <span class="info-value">{data.with_description.to_string()}</span>
                                        </div>
                                        <div class="info-row">
                                            <span class="info-label">{t(locale, "metadata.with_rating")}</span>
                                            <span class="info-value">{data.with_rating.to_string()}</span>
                                        </div>
                                        <div class="info-row">
                                            <span class="info-label">{t(locale, "metadata.db_size")}</span>
                                            <span class="info-value">{format_size(data.db_size_bytes)}</span>
                                        </div>
                                    </div>
                                }.into_any()
                            })
                        })}
                    </Suspense>
                </ErrorBoundary>
            </section>

            // Import section
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.import")}</h2>
                <p class="settings-hint">{move || t(i18n.locale.get(), "metadata.import_hint")}</p>
                <div class="metadata-import-form">
                    <input
                        type="text"
                        class="rename-input"
                        placeholder="/path/to/Metadata.xml"
                        bind:value=xml_path
                    />
                    <button
                        class="game-action-btn"
                        on:click=on_import
                        disabled=move || importing.get() || xml_path.read().is_empty()
                    >
                        {move || if importing.get() {
                            t(i18n.locale.get(), "metadata.importing")
                        } else {
                            t(i18n.locale.get(), "metadata.import_launchbox")
                        }}
                    </button>
                </div>
                <Show when=move || import_result.get().is_some()>
                    <p class="settings-saved">{move || import_result.get().unwrap_or_default()}</p>
                </Show>
            </section>

            // Cache management section
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.clear")}</h2>
                <Show when=move || confirming_clear.get()
                    fallback=move || view! {
                        <button
                            class="game-action-btn game-action-delete"
                            on:click=move |_| confirming_clear.set(true)
                        >
                            {move || t(i18n.locale.get(), "metadata.clear")}
                        </button>
                    }
                >
                    <p class="settings-hint">{move || t(i18n.locale.get(), "metadata.confirm_clear")}</p>
                    <div class="game-delete-confirm">
                        <button
                            class="game-action-btn game-action-delete-confirm"
                            on:click=on_clear
                            disabled=move || clearing.get()
                        >
                            {move || if clearing.get() {
                                t(i18n.locale.get(), "metadata.clearing")
                            } else {
                                t(i18n.locale.get(), "metadata.clear")
                            }}
                        </button>
                        <button class="game-action-btn" on:click=move |_| confirming_clear.set(false)>
                            {move || t(i18n.locale.get(), "games.cancel")}
                        </button>
                    </div>
                </Show>
                <Show when=move || clear_result.get().is_some()>
                    <p class="settings-saved">{move || clear_result.get().unwrap_or_default()}</p>
                </Show>
            </section>

            // Attribution section
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.attribution")}</h2>
                <p class="settings-hint">{move || t(i18n.locale.get(), "metadata.attribution_text")}</p>
            </section>
        </div>
    }
}
