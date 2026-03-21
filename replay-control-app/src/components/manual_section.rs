use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

use crate::i18n::{t, use_i18n};
use crate::server_fns::{self, GameDocument, LocalManual, ManualRecommendation};

/// Full manual section: in-folder documents, saved manuals, search, and results.
///
/// Resources use URL params as their reactive key so they refetch automatically
/// when the user navigates between games via client-side navigation.
#[component]
pub fn ManualSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    display_name: StoredValue<String>,
    base_title: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    // Use URL params as the reactive key for Resources. This ensures they
    // refetch when the user navigates between games (StoredValue::get_value()
    // is not reactive, so it can't drive Resource refetches on its own).
    let params = use_params_map();
    let param_key = Memo::new(move |_| {
        let p = params.read();
        let sys = p.get("system").unwrap_or_default();
        let fname = p.get("filename").unwrap_or_default();
        (sys, fname)
    });

    // In-folder documents (Phase 1)
    let docs_resource = Resource::new(
        move || param_key.get(),
        move |_| {
            let sys = system.get_value();
            let fname = rom_filename.get_value();
            server_fns::get_game_documents(sys, fname)
        },
    );

    let game_docs = RwSignal::new(Vec::<GameDocument>::new());

    let _sync_docs = Effect::new(move || {
        if let Some(Ok(docs)) = docs_resource.get() {
            game_docs.set(docs);
        }
    });

    // Local manuals (Phase 2 -- previously downloaded)
    let local_resource = Resource::new(
        move || param_key.get(),
        move |_| {
            let sys = system.get_value();
            let bt = base_title.get_value();
            server_fns::get_local_manuals(sys, bt)
        },
    );

    let local_manuals = RwSignal::new(Vec::<LocalManual>::new());

    let _sync_local = Effect::new(move || {
        if let Some(Ok(manuals)) = local_resource.get() {
            local_manuals.set(manuals);
        }
    });

    // Search state (Phase 2)
    let search_results = RwSignal::new(Vec::<ManualRecommendation>::new());
    let searching = RwSignal::new(false);
    let search_error = RwSignal::new(Option::<String>::None);
    let searched = RwSignal::new(false);

    // Download state
    let downloading_url = RwSignal::new(Option::<String>::None);
    let download_error = RwSignal::new(Option::<String>::None);

    // Delete state
    let deleting_filename = RwSignal::new(Option::<String>::None);
    let confirming_delete = RwSignal::new(Option::<String>::None);

    // Reset search state when the game changes (driven by URL params)
    let _reset_search = Effect::new(move || {
        let _ = param_key.get(); // track the reactive key
        searched.set(false);
        search_results.set(vec![]);
        search_error.set(None);
        searching.set(false);
        downloading_url.set(None);
        download_error.set(None);
    });

    let on_search = move |_| {
        searching.set(true);
        search_error.set(None);
        searched.set(true);
        search_results.set(vec![]);

        let sys = system.get_value();
        let bt = base_title.get_value();
        let dn = display_name.get_value();
        leptos::task::spawn_local(async move {
            match server_fns::search_game_manuals(sys, bt, dn).await {
                Ok(results) => search_results.set(results),
                Err(e) => search_error.set(Some(e.to_string())),
            }
            searching.set(false);
        });
    };

    let on_download = move |rec: ManualRecommendation| {
        let sys = system.get_value();
        let bt = base_title.get_value();
        let url = rec.url.clone();
        let lang = rec.language.clone();

        downloading_url.set(Some(url.clone()));
        download_error.set(None);

        // Clone values needed after the download await. We must NOT access
        // StoredValue after an await — if the user navigates away while the
        // download is in progress, the component's reactive owner is disposed
        // and StoredValue::get_value() would panic, freezing the page.
        let sys_for_refresh = sys.clone();
        let bt_for_refresh = bt.clone();

        leptos::task::spawn_local(async move {
            match server_fns::download_manual(sys, bt, url.clone(), lang).await {
                Ok(_serve_url) => {
                    // Refresh local manuals list
                    if let Ok(manuals) =
                        server_fns::get_local_manuals(sys_for_refresh, bt_for_refresh).await
                    {
                        local_manuals.set(manuals);
                    }
                }
                Err(e) => {
                    download_error.set(Some(e.to_string()));
                }
            }
            downloading_url.set(None);
        });
    };

    let on_delete = move |filename: String| {
        let sys = system.get_value();
        deleting_filename.set(Some(filename.clone()));

        // Clone values needed after the await (same safety pattern as on_download).
        let sys_for_refresh = sys.clone();
        let bt_for_refresh = base_title.get_value();

        leptos::task::spawn_local(async move {
            match server_fns::delete_manual(sys, filename).await {
                Ok(()) => {
                    // Refresh local manuals list
                    if let Ok(manuals) =
                        server_fns::get_local_manuals(sys_for_refresh, bt_for_refresh).await
                    {
                        local_manuals.set(manuals);
                    }
                }
                Err(e) => {
                    download_error.set(Some(e.to_string()));
                }
            }
            deleting_filename.set(None);
        });
    };

    let has_docs = move || !game_docs.read().is_empty();
    let has_local = move || !local_manuals.read().is_empty();
    let has_content = move || has_docs() || has_local();

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.manual")}</h2>

            // In-folder documents
            <Show when=has_docs>
                <div class="manual-list">
                    <For
                        each=move || game_docs.get()
                        key=|doc| doc.relative_path.clone()
                        let:doc
                    >
                        <DocumentLink doc=doc system=system rom_filename=rom_filename />
                    </For>
                </div>
            </Show>

            // Local saved manuals
            <Show when=has_local>
                <div class="manual-list">
                    <For
                        each=move || local_manuals.get()
                        key=|m| m.filename.clone()
                        let:manual
                    >
                        <LocalManualLink
                            manual=manual
                            deleting_filename=deleting_filename
                            confirming_delete=confirming_delete
                            on_delete=on_delete.clone()
                        />
                    </For>
                </div>
            </Show>

            // No manual message
            <Show when=move || !has_content() && !searched.get()>
                <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_manual")}</p>
            </Show>

            // Search button
            <button
                class="game-action-btn"
                prop:disabled=move || searching.get()
                on:click=on_search
            >
                {move || {
                    if searching.get() {
                        t(i18n.locale.get(), "game_detail.searching_manual")
                    } else {
                        t(i18n.locale.get(), "game_detail.find_manual")
                    }
                }}
            </button>

            // Search error
            <Show when=move || search_error.get().is_some()>
                <p class="manual-error">{move || search_error.get().unwrap_or_default()}</p>
            </Show>

            // Download error
            <Show when=move || download_error.get().is_some()>
                <p class="manual-error">{move || download_error.get().unwrap_or_default()}</p>
            </Show>

            // Search results
            <Show when=move || searched.get()>
                <ManualSearchResults
                    results=search_results
                    is_searching=searching
                    downloading_url=downloading_url
                    on_download=on_download
                />
            </Show>
        </section>
    }
}

/// A single in-folder document link.
#[component]
fn DocumentLink(
    doc: GameDocument,
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
) -> impl IntoView {
    let icon = match doc.extension.as_str() {
        "pdf" => "\u{1F4C4}", // page facing up
        "txt" => "\u{1F4DD}", // memo
        "jpg" | "jpeg" | "png" | "gif" => "\u{1F5BC}", // framed picture
        "html" | "htm" => "\u{1F310}", // globe with meridians
        _ => "\u{1F4CE}",     // paperclip
    };

    let size_display = format_file_size(doc.size_bytes);
    let label = doc.label.clone();
    let ext_upper = doc.extension.to_uppercase();

    // Build the URL using base64-encoded ROM filename for path safety
    let encoded_rom = StoredValue::new(
        crate::util::base64_encode(rom_filename.get_value().as_bytes()),
    );
    let encoded_path = StoredValue::new(urlencoding::encode(&doc.relative_path).to_string());
    let sys = system;

    let href = move || {
        format!(
            "/rom-docs/{}/{}/{}",
            sys.get_value(),
            encoded_rom.get_value(),
            encoded_path.get_value()
        )
    };

    view! {
        <a class="manual-link" href=href target="_blank" rel="noopener">
            <span class="manual-icon">{icon}</span>
            <span class="manual-info">
                <span class="manual-label">{label}</span>
                <span class="manual-meta">{ext_upper}" \u{00B7} "{size_display}</span>
            </span>
        </a>
    }
}

/// A single local manual link (downloaded PDF) with inline delete confirmation.
#[component]
fn LocalManualLink<F>(
    manual: LocalManual,
    deleting_filename: RwSignal<Option<String>>,
    confirming_delete: RwSignal<Option<String>>,
    on_delete: F,
) -> impl IntoView
where
    F: Fn(String) + Clone + Send + Sync + 'static,
{
    let i18n = use_i18n();
    let size_display = format_file_size(manual.size_bytes);
    let label = manual.label.clone();
    let lang_display = manual
        .language
        .as_deref()
        .map(|l| format!(" ({l})"))
        .unwrap_or_default();

    let filename = StoredValue::new(manual.filename.clone());
    let is_deleting = move || {
        deleting_filename
            .read()
            .as_ref()
            .is_some_and(|f| *f == filename.get_value())
    };
    let is_confirming = move || {
        confirming_delete
            .read()
            .as_ref()
            .is_some_and(|f| *f == filename.get_value())
    };

    let on_delete_sv = StoredValue::new(on_delete);

    // First click: enter confirmation mode
    let on_click_delete = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        ev.stop_propagation();
        confirming_delete.set(Some(filename.get_value()));
    };

    // Confirm: actually delete
    let on_click_confirm = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        ev.stop_propagation();
        confirming_delete.set(None);
        (on_delete_sv.get_value())(filename.get_value());
    };

    // Cancel: revert to normal state
    let on_click_cancel = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        ev.stop_propagation();
        confirming_delete.set(None);
    };

    let url = manual.url.clone();

    view! {
        <div class="manual-link-row">
            <a class="manual-link" href=url target="_blank" rel="noopener">
                <span class="manual-icon">{"\u{1F4C4}"}</span>
                <span class="manual-info">
                    <span class="manual-label">{label}{lang_display}</span>
                    <span class="manual-meta">"PDF \u{00B7} "{size_display}</span>
                </span>
            </a>
            <Show when=move || !is_confirming()>
                <button
                    class="manual-delete-btn"
                    prop:disabled=is_deleting
                    on:click=on_click_delete
                    title="Delete manual"
                >
                    "\u{00D7}"
                </button>
            </Show>
            <Show when=is_confirming>
                <button
                    class="manual-confirm-btn"
                    on:click=on_click_confirm
                >
                    {move || t(i18n.locale.get(), "manual.confirm_delete")}
                </button>
                <button
                    class="manual-cancel-btn"
                    on:click=on_click_cancel
                >
                    {move || t(i18n.locale.get(), "manual.cancel")}
                </button>
            </Show>
        </div>
    }
}

/// Panel showing manual search results.
#[component]
fn ManualSearchResults<F>(
    results: RwSignal<Vec<ManualRecommendation>>,
    is_searching: RwSignal<bool>,
    downloading_url: RwSignal<Option<String>>,
    on_download: F,
) -> impl IntoView
where
    F: Fn(ManualRecommendation) + Clone + Send + Sync + 'static,
{
    let i18n = use_i18n();

    view! {
        <div class="manual-search-results">
            <Show when=move || !is_searching.get() && results.read().is_empty()>
                <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_manual_results")}</p>
            </Show>
            <For
                each=move || results.get()
                key=|rec| format!("{}:{}", rec.source, rec.url)
                let:rec
            >
                <ManualResultItem
                    rec=rec.clone()
                    downloading_url=downloading_url
                    on_download=on_download.clone()
                />
            </For>
        </div>
    }
}

/// A single manual search result with View/Save buttons.
#[component]
fn ManualResultItem<F>(
    rec: ManualRecommendation,
    downloading_url: RwSignal<Option<String>>,
    on_download: F,
) -> impl IntoView
where
    F: Fn(ManualRecommendation) + Clone + Send + Sync + 'static,
{
    let i18n = use_i18n();
    let rec_sv = StoredValue::new(rec.clone());

    let is_downloading = move || {
        downloading_url
            .read()
            .as_ref()
            .is_some_and(|u| *u == rec_sv.get_value().url)
    };

    let on_download_sv = StoredValue::new(on_download);

    let on_click_save = move |_| {
        let r = rec_sv.get_value();
        (on_download_sv.get_value())(r);
    };

    let meta_parts = {
        let mut parts = Vec::new();
        parts.push(rec.source.clone());
        if let Some(ref lang) = rec.language {
            parts.push(lang.clone());
        }
        if let Some(size) = rec.size_bytes {
            parts.push(format_file_size(size));
        }
        parts.join(" \u{00B7} ")
    };

    let is_retrokit = rec.source == "retrokit";

    view! {
        <div class="manual-result-item">
            <div class="manual-result-info">
                <div class="manual-result-title">{rec.title.clone()}</div>
                <div class="manual-result-meta">{meta_parts}</div>
            </div>
            <div class="manual-result-actions">
                <a
                    class="manual-result-btn"
                    href=rec.url.clone()
                    target="_blank"
                    rel="noopener"
                >
                    {move || t(i18n.locale.get(), "game_detail.view_manual")}
                </a>
                // Only show Save for retrokit results (direct PDF URLs)
                <Show when=move || is_retrokit>
                    <button
                        class="manual-result-btn manual-save-btn"
                        prop:disabled=is_downloading
                        on:click=on_click_save
                    >
                        {move || {
                            if is_downloading() {
                                t(i18n.locale.get(), "game_detail.downloading")
                            } else {
                                t(i18n.locale.get(), "game_detail.save_manual")
                            }
                        }}
                    </button>
                </Show>
            </div>
        </div>
    }
}

/// Format a file size in human-readable form.
fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
