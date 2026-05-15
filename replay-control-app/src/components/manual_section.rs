use leptos::{html, prelude::*};

use crate::hooks::use_focus_scroll;
use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, GameDocument, LocalManual, ManualRecommendation};

/// Full manual section: in-folder documents, saved manuals, suggestions, and add actions.
///
/// This component is inside the game detail `<Suspense>`, so it gets recreated
/// with fresh props whenever the user navigates between games. No need for
/// reactive URL param tracking — all state starts fresh per game.
#[component]
pub fn ManualSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    display_name: StoredValue<String>,
    base_title: StoredValue<String>,
    #[prop(optional)] section_id: Option<&'static str>,
    focus_on_mount: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let section_ref = NodeRef::<html::Section>::new();

    // In-folder documents (Phase 1).
    // Resource fires once when the component is created (key is constant).
    let docs_resource = Resource::new(
        || (),
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
        || (),
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

    let manual_suggestions = RwSignal::new(Vec::<ManualRecommendation>::new());
    let suggestions_loading = RwSignal::new(false);

    // Bundled/library manual suggestions should appear without forcing the
    // user to run an online search. This server fn only reads library.db and
    // never falls back to Archive.org.
    let suggestions_resource = Resource::new(
        || (),
        move |_| {
            let sys = system.get_value();
            let fname = rom_filename.get_value();
            let bt = base_title.get_value();
            server_fns::get_game_manual_suggestions(sys, fname, bt)
        },
    );

    let _sync_suggestions = Effect::new(move || {
        if let Some(Ok(results)) = suggestions_resource.get() {
            manual_suggestions.set(results);
        }
    });

    // Download state
    let downloading_url = RwSignal::new(Option::<String>::None);
    let download_error = RwSignal::new(Option::<String>::None);
    let add_url = RwSignal::new(String::new());
    let adding_url = RwSignal::new(false);
    let upload_input_ref = NodeRef::<html::Input>::new();
    let uploading = RwSignal::new(false);
    let add_success = RwSignal::new(false);

    // Delete state
    let deleting_filename = RwSignal::new(Option::<String>::None);
    let confirming_delete = RwSignal::new(Option::<String>::None);

    let on_download = move |rec: ManualRecommendation| {
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        let bt = base_title.get_value();
        let url = rec.url.clone();
        let lang = rec.language.clone();
        let title = Some(rec.title.clone());
        let source = Some(rec.source.clone());

        downloading_url.set(Some(url.clone()));
        download_error.set(None);

        // Clone values needed after the download await. We must NOT access
        // StoredValue after an await — if the user navigates away while the
        // download is in progress, the component's reactive owner is disposed
        // and StoredValue::get_value() would panic, freezing the page.
        let sys_for_refresh = sys.clone();
        let bt_for_refresh = bt.clone();

        leptos::task::spawn_local(async move {
            match server_fns::download_manual(sys, fname, bt, url.clone(), lang, title, source)
                .await
            {
                Ok(_serve_url) => {
                    manual_suggestions.update(|results| results.retain(|result| result.url != url));
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

    let on_add_url = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        let url = add_url.read().trim().to_string();
        if url.is_empty() {
            return;
        }
        adding_url.set(true);
        download_error.set(None);
        add_success.set(false);

        let sys = system.get_value();
        let fname = rom_filename.get_value();
        let bt = base_title.get_value();
        let title = Some(display_name.get_value());
        let sys_for_refresh = sys.clone();
        let bt_for_refresh = bt.clone();

        leptos::task::spawn_local(async move {
            match server_fns::download_manual(
                sys,
                fname,
                bt,
                url.clone(),
                None,
                title,
                Some("user_url".to_string()),
            )
            .await
            {
                Ok(_) => {
                    add_url.set(String::new());
                    add_success.set(true);
                    manual_suggestions.update(|results| results.retain(|result| result.url != url));
                    if let Ok(manuals) =
                        server_fns::get_local_manuals(sys_for_refresh, bt_for_refresh).await
                    {
                        local_manuals.set(manuals);
                    }
                }
                Err(e) => download_error.set(Some(e.to_string())),
            }
            adding_url.set(false);
        });
    };

    let on_upload = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        download_error.set(None);
        add_success.set(false);

        let sys = system.get_value();
        let fname = rom_filename.get_value();
        let bt = base_title.get_value();
        let title = display_name.get_value();
        let sys_for_refresh = sys.clone();
        let bt_for_refresh = bt.clone();

        #[cfg(target_arch = "wasm32")]
        {
            let choose_file_message =
                t(i18n.locale.get_untracked(), Key::GameDetailManualChooseFile);
            let Some(input) = upload_input_ref.get() else {
                download_error.set(Some(choose_file_message.to_string()));
                return;
            };
            let Some(files) = input.files() else {
                download_error.set(Some(choose_file_message.to_string()));
                return;
            };
            let Some(file) = files.item(0) else {
                download_error.set(Some(choose_file_message.to_string()));
                return;
            };
            let filename = file.name();
            let lower = filename.to_lowercase();
            if !(lower.ends_with(".pdf") || lower.ends_with(".txt")) {
                download_error.set(Some(
                    t(
                        i18n.locale.get_untracked(),
                        Key::GameDetailManualInvalidFileType,
                    )
                    .to_string(),
                ));
                return;
            }

            uploading.set(true);
            leptos::task::spawn_local(async move {
                match upload_manual_file(&sys, &fname, &bt, &title, file).await {
                    Ok(()) => {
                        input.set_value("");
                        add_success.set(true);
                        if let Ok(manuals) =
                            server_fns::get_local_manuals(sys_for_refresh, bt_for_refresh).await
                        {
                            local_manuals.set(manuals);
                        }
                    }
                    Err(e) => download_error.set(Some(e)),
                }
                uploading.set(false);
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (sys, fname, bt, title, sys_for_refresh, bt_for_refresh);
            download_error.set(Some(
                t(
                    i18n.locale.get_untracked(),
                    Key::GameDetailManualUploadBrowserOnly,
                )
                .to_string(),
            ));
        }
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
    let has_suggestions = Signal::derive(move || {
        !visible_manual_recommendations(&manual_suggestions.read(), &local_manuals.read())
            .is_empty()
    });
    let displayed_results = Signal::derive(move || {
        visible_manual_recommendations(&manual_suggestions.read(), &local_manuals.read())
    });

    use_focus_scroll(section_ref, move || focus_on_mount.get());

    view! {
        <section node_ref=section_ref id=section_id class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameDetailManual)}</h2>

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
                            on_delete=on_delete
                        />
                    </For>
                </div>
            </Show>

            // No manual message
            <Show when=move || !has_content() && !has_suggestions.get()>
                <p class="game-section-empty">{move || t(i18n.locale.get(), Key::GameDetailNoManual)}</p>
            </Show>

            // Download error
            <Show when=move || download_error.get().is_some()>
                <p class="manual-error">{move || download_error.get().unwrap_or_default()}</p>
            </Show>
            <Show when=move || add_success.get()>
                <p class="manual-success">{move || t(i18n.locale.get(), Key::GameDetailManualSaved)}</p>
            </Show>

            // Catalog/manual suggestions
            <Show when=move || has_suggestions.get()>
                <h3 class="manual-subsection-title">
                    {move || t(i18n.locale.get(), Key::GameDetailSuggestedManuals)}
                </h3>
                <ManualSearchResults
                    results=displayed_results
                    is_searching=suggestions_loading
                    downloading_url=downloading_url
                    on_download=on_download
                />
            </Show>

            <h3 class="manual-subsection-title">
                {move || t(i18n.locale.get(), Key::GameDetailAddManual)}
            </h3>
            <div class="manual-add-form">
                <input
                    type="text"
                    class="form-input"
                    placeholder=move || t(i18n.locale.get(), Key::GameDetailManualUrlPlaceholder)
                    prop:value=move || add_url.get()
                    on:input=move |ev| {
                        add_url.set(event_target_value(&ev));
                        download_error.set(None);
                        add_success.set(false);
                    }
                    on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                        if ev.key() == "Enter" {
                            ev.prevent_default();
                            let url = add_url.read().trim().to_string();
                            if !url.is_empty() {
                                adding_url.set(true);
                                download_error.set(None);
                                add_success.set(false);
                                let sys = system.get_value();
                                let fname = rom_filename.get_value();
                                let bt = base_title.get_value();
                                let title = Some(display_name.get_value());
                                let sys_for_refresh = sys.clone();
                                let bt_for_refresh = bt.clone();
                                leptos::task::spawn_local(async move {
                                    match server_fns::download_manual(sys, fname, bt, url.clone(), None, title, Some("user_url".to_string())).await {
                                        Ok(_) => {
                                            add_url.set(String::new());
                                            add_success.set(true);
                                            manual_suggestions.update(|results| results.retain(|result| result.url != url));
                                            if let Ok(manuals) =
                                                server_fns::get_local_manuals(sys_for_refresh, bt_for_refresh).await
                                            {
                                                local_manuals.set(manuals);
                                            }
                                        }
                                        Err(e) => download_error.set(Some(e.to_string())),
                                    }
                                    adding_url.set(false);
                                });
                            }
                        }
                    }
                />
                <button
                    type="button"
                    class="game-action-btn"
                    prop:disabled=move || adding_url.get() || add_url.read().trim().is_empty()
                    on:click=on_add_url
                >
                    {move || {
                        if adding_url.get() {
                            t(i18n.locale.get(), Key::GameDetailDownloading)
                        } else {
                            t(i18n.locale.get(), Key::CommonSave)
                        }
                    }}
                </button>
            </div>
            <div class="manual-upload-form">
                <input
                    node_ref=upload_input_ref
                    type="file"
                    class="manual-file-input"
                    accept=".pdf,.txt,application/pdf,text/plain"
                    on:change=move |_| {
                        download_error.set(None);
                        add_success.set(false);
                    }
                />
                <button
                    type="button"
                    class="game-action-btn"
                    prop:disabled=move || uploading.get()
                    on:click=on_upload
                >
                    {move || {
                        if uploading.get() {
                            t(i18n.locale.get(), Key::CommonUpdating)
                        } else {
                            t(i18n.locale.get(), Key::GameDetailUploadManual)
                        }
                    }}
                </button>
            </div>
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
        "pdf" => "\u{1F4C4}",                          // page facing up
        "txt" => "\u{1F4DD}",                          // memo
        "jpg" | "jpeg" | "png" | "gif" => "\u{1F5BC}", // framed picture
        "html" | "htm" => "\u{1F310}",                 // globe with meridians
        _ => "\u{1F4CE}",                              // paperclip
    };

    let size_display = crate::util::format_size(doc.size_bytes);
    let label = doc.label.clone();
    let ext_upper = doc.extension.to_uppercase();

    // Build the URL using base64-encoded ROM filename for path safety
    let encoded_rom = StoredValue::new(crate::util::base64_encode(
        rom_filename.get_value().as_bytes(),
    ));
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
    let size_display = crate::util::format_size(manual.size_bytes);
    let label = manual_title_with_language(&manual.label, manual.language.as_deref());
    let kind = manual_file_kind(&manual.filename);
    let source = if let Some(source_url) = manual.source_url.as_deref() {
        Some(manual_source_meta(source_url, manual.provider.as_deref()))
    } else {
        manual
            .provider
            .as_deref()
            .map(provider_label)
            .map(str::to_string)
    };
    let meta = if let Some(source) = source {
        format!("{source} \u{00B7} {kind} \u{00B7} {size_display}")
    } else {
        format!("{kind} \u{00B7} {size_display}")
    };

    let delete_id = manual.delete_id.clone();
    let delete_key = StoredValue::new(delete_id.clone().unwrap_or_default());
    let is_deleting = move || {
        deleting_filename
            .read()
            .as_ref()
            .is_some_and(|f| *f == delete_key.get_value())
    };
    let is_confirming = move || {
        confirming_delete
            .read()
            .as_ref()
            .is_some_and(|f| *f == delete_key.get_value())
    };

    let on_delete_sv = StoredValue::new(on_delete);

    // First click: enter confirmation mode
    let on_click_delete = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        ev.stop_propagation();
        confirming_delete.set(Some(delete_key.get_value()));
    };

    // Confirm: actually delete
    let on_click_confirm = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        ev.stop_propagation();
        confirming_delete.set(None);
        (on_delete_sv.get_value())(delete_key.get_value());
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
                    <span class="manual-label">{label}</span>
                    <span class="manual-meta">{meta}</span>
                </span>
            </a>
            <Show when=move || delete_id.is_some()>
                <Show when=move || !is_confirming()>
                    <button
                        type="button"
                        class="manual-delete-btn"
                        prop:disabled=is_deleting
                        on:click=on_click_delete
                        title="Delete manual"
                    >
                        "\u{00D7}"
                    </button>
                </Show>
            </Show>
            <Show when=is_confirming>
                <button
                    type="button"
                    class="manual-confirm-btn"
                    on:click=on_click_confirm
                >
                    {move || t(i18n.locale.get(), Key::ManualConfirmDelete)}
                </button>
                <button
                    type="button"
                    class="manual-cancel-btn"
                    on:click=on_click_cancel
                >
                    {move || t(i18n.locale.get(), Key::CommonCancel)}
                </button>
            </Show>
        </div>
    }
}

/// Panel showing manual search results.
#[component]
fn ManualSearchResults<F>(
    #[prop(into)] results: Signal<Vec<ManualRecommendation>>,
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
                <p class="game-section-empty">{move || t(i18n.locale.get(), Key::GameDetailNoManualResults)}</p>
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

    let title = manual_title_with_language(&rec.title, rec.language.as_deref());
    let meta = {
        let source = manual_source_meta(&rec.url, Some(&rec.source));
        if let Some(size) = rec.size_bytes {
            format!("{source} \u{00B7} {}", crate::util::format_size(size))
        } else {
            source
        }
    };

    view! {
        <div class="manual-result-item">
            <div class="manual-result-info">
                <div class="manual-result-title">{title}</div>
                <div class="manual-result-meta">{meta}</div>
            </div>
            <div class="manual-result-actions">
                <a
                    class="manual-result-btn"
                    href=rec.url.clone()
                    target="_blank"
                    rel="noopener"
                >
                    {move || t(i18n.locale.get(), Key::GameDetailViewManual)}
                </a>
                <button
                    type="button"
                    class="manual-result-btn manual-save-btn"
                    prop:disabled=is_downloading
                    on:click=on_click_save
                >
                    {move || {
                        if is_downloading() {
                            t(i18n.locale.get(), Key::GameDetailDownloading)
                        } else {
                            t(i18n.locale.get(), Key::CommonSave)
                        }
                    }}
                </button>
            </div>
        </div>
    }
}

fn visible_manual_recommendations(
    results: &[ManualRecommendation],
    local_manuals: &[LocalManual],
) -> Vec<ManualRecommendation> {
    results
        .iter()
        .filter(|rec| !is_manual_saved(rec, local_manuals))
        .cloned()
        .collect()
}

fn is_manual_saved(rec: &ManualRecommendation, local_manuals: &[LocalManual]) -> bool {
    local_manuals
        .iter()
        .filter_map(|manual| manual.source_url.as_deref())
        .any(|source_url| source_url == rec.url)
}

#[cfg(target_arch = "wasm32")]
async fn upload_manual_file(
    system: &str,
    rom_filename: &str,
    base_title: &str,
    title: &str,
    file: web_sys::File,
) -> Result<(), String> {
    use wasm_bindgen::JsCast;

    let form = web_sys::FormData::new().map_err(|_| "Could not prepare upload.".to_string())?;
    form.append_with_str("rom_filename", rom_filename)
        .map_err(|_| "Could not prepare upload.".to_string())?;
    form.append_with_str("base_title", base_title)
        .map_err(|_| "Could not prepare upload.".to_string())?;
    form.append_with_str("title", title)
        .map_err(|_| "Could not prepare upload.".to_string())?;
    form.append_with_blob_and_filename("file", file.as_ref(), &file.name())
        .map_err(|_| "Could not attach manual file.".to_string())?;

    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_body(&form);

    let Some(window) = web_sys::window() else {
        return Err("Browser window unavailable.".to_string());
    };
    let response = wasm_bindgen_futures::JsFuture::from(
        window.fetch_with_str_and_init(&format!("/api/manuals/upload/{system}"), &init),
    )
    .await
    .map_err(|_| "Manual upload failed.".to_string())?;
    let response: web_sys::Response = response
        .dyn_into()
        .map_err(|_| "Manual upload failed.".to_string())?;
    if response.ok() {
        Ok(())
    } else {
        Err("Manual upload failed. Use a PDF or text file.".to_string())
    }
}

fn manual_source_meta(url: &str, provider: Option<&str>) -> String {
    let domain = manual_url_domain(url);
    let provider = provider.unwrap_or_default().trim();
    if provider.is_empty() || provider.eq_ignore_ascii_case(&domain) {
        domain
    } else {
        format!("{domain} ({})", provider_label(provider))
    }
}

fn provider_label(provider: &str) -> &str {
    match provider {
        "user_url" => "User URL",
        "user_upload" => "User upload",
        "mister_manuals" => "MiSTer Manuals",
        "retrokit" => "Retrokit",
        "archive_org" | "archive.org" => "Archive.org",
        other => other,
    }
}

fn manual_file_kind(filename: &str) -> &'static str {
    if filename.to_lowercase().ends_with(".txt") {
        "TXT"
    } else {
        "PDF"
    }
}

fn manual_url_domain(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    without_scheme
        .split('/')
        .next()
        .unwrap_or_default()
        .trim_start_matches("www.")
        .to_string()
}

fn manual_title_with_language(title: &str, language: Option<&str>) -> String {
    let Some(language) = language.map(str::trim).filter(|l| !l.is_empty()) else {
        return title.to_string();
    };
    if title_mentions_language(title, language) {
        title.to_string()
    } else {
        format!("{title} ({language})")
    }
}

fn title_mentions_language(title: &str, language: &str) -> bool {
    let lower_title = title.to_lowercase();
    let normalized_title = format!(" {} ", lower_title.replace(['_', '-', '.', ','], " "));
    language
        .split([',', ';', '/', '|'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .any(|part| {
            let part = part.to_lowercase();
            let aliases = language_aliases(&part);
            aliases.iter().any(|alias| {
                let alias = alias.to_lowercase();
                lower_title.contains(&format!("({alias})"))
                    || lower_title.contains(&format!("[{alias}]"))
                    || normalized_title.contains(&format!(" {alias} "))
            })
        })
}

fn language_aliases<'a>(language: &'a str) -> Vec<&'a str> {
    match language {
        "en" | "eng" | "english" => vec!["en", "eng", "english"],
        "es" | "spa" | "spanish" => vec!["es", "spa", "spanish", "espanol", "español"],
        "fr" | "fre" | "fra" | "french" => vec!["fr", "fre", "fra", "french", "francais"],
        "de" | "ger" | "deu" | "german" => vec!["de", "ger", "deu", "german", "deutsch"],
        "it" | "ita" | "italian" => vec!["it", "ita", "italian", "italiano"],
        "ja" | "jp" | "jpn" | "japanese" => vec!["ja", "jp", "jpn", "japanese"],
        "pt" | "por" | "portuguese" => vec!["pt", "por", "portuguese"],
        "nl" | "dut" | "nld" | "dutch" => vec!["nl", "dut", "nld", "dutch"],
        _ => vec![language],
    }
}
