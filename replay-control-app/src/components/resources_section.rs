use leptos::{html, prelude::*};
use replay_control_core::library_db::LibraryResourceLink;
use replay_control_core::video_url;
use server_fn::ServerFnError;
use std::collections::HashSet;

use crate::i18n::{Key, Locale, t, use_i18n};
use crate::server_fns::{
    self, GameDocument, LocalManual, ManualRecommendation, ResourceEntry, VideoEntry,
    VideoRecommendation,
};

const COLLAPSED_SUGGESTION_LINK_COUNT: usize = 2;
const COLLAPSED_SUGGESTION_VIDEO_COUNT: usize = 2;
#[cfg(target_arch = "wasm32")]
const RESOURCE_SUCCESS_VISIBLE_MS: u32 = 3_000;

#[derive(Clone)]
struct ResourceLink {
    title: String,
    meta: String,
    href: String,
    icon: &'static str,
    action: ResourceAction,
}

#[derive(Clone)]
enum ResourceAction {
    None,
    SaveManual {
        url: String,
        language: Option<String>,
        title: String,
        source: String,
    },
    SaveLink {
        url: String,
        title: String,
        source: Option<String>,
        resource_type: String,
    },
    RemoveManual {
        delete_id: String,
        href: String,
        source_url: Option<String>,
    },
    RemoveLink {
        id: String,
        rom_filename: String,
        href: String,
    },
}

#[derive(Clone)]
struct ResourceVideo {
    title: String,
    meta: String,
    href: String,
    thumbnail_url: Option<String>,
    embed_url: Option<String>,
    action: VideoResourceAction,
}

#[derive(Clone)]
enum VideoResourceAction {
    Pin {
        video: VideoRecommendation,
        tag: String,
    },
    Remove {
        id: String,
        rom_filename: String,
        href: String,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResourceStatusKind {
    Success,
    Warning,
    Error,
}

/// The kinds of online video search offered for a game.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VideoSearchKind {
    Trailer,
    Gameplay,
    OneCc,
}

impl VideoSearchKind {
    fn query_type(self) -> &'static str {
        match self {
            Self::Trailer => "trailer",
            Self::Gameplay => "gameplay",
            Self::OneCc => "1cc",
        }
    }

    fn label_key(self) -> Key {
        match self {
            Self::Trailer => Key::GameDetailFindTrailers,
            Self::Gameplay => Key::GameDetailFindGameplay,
            Self::OneCc => Key::GameDetailFind1cc,
        }
    }
}

#[component]
pub fn GameResourcesSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    base_title: StoredValue<String>,
    display_name: StoredValue<String>,
    library_resources: StoredValue<Vec<LibraryResourceLink>>,
    // Resources-section data bundled in the `get_rom_detail` payload (see
    // RomDetail). The resources below resolve from these instead of issuing
    // their own per-section fetches, so a client-side navigation makes one
    // request (the page) rather than six fragile ones.
    initial_documents: StoredValue<Vec<GameDocument>>,
    initial_local_manuals: StoredValue<Vec<LocalManual>>,
    initial_saved_videos: StoredValue<Vec<VideoEntry>>,
    initial_saved_resource_links: StoredValue<Vec<ResourceEntry>>,
    initial_manual_suggestions: StoredValue<Vec<ManualRecommendation>>,
    initial_video_suggestions: StoredValue<Vec<VideoRecommendation>>,
    #[prop(optional)] section_id: Option<&'static str>,
    focus_on_mount: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let section_ref = NodeRef::<html::Section>::new();
    crate::hooks::use_focus_scroll(section_ref, move || focus_on_mount.get());

    // The six resource lists are loaded once with the page in `get_rom_detail`
    // and handed in as props (see RomDetail / GameResourcesSection docs). Each
    // resource below simply resolves from its prop instead of issuing its own
    // server-fn fetch. This keeps the section server-rendered on a full load and
    // -- crucially -- makes a client-side navigation carry the resources in the
    // single `get_rom_detail` request, instead of six separate per-section
    // fetches that could each silently drop over an imperfect network. Wrapping
    // the prop in a resolved Resource keeps every downstream reader (caches,
    // Effects, derived closures, Suspense) unchanged.
    let docs_resource = Resource::new_blocking(
        || (),
        move |_| {
            let v = initial_documents.get_value();
            async move { Ok::<_, ServerFnError>(v) }
        },
    );
    let local_manuals_resource = Resource::new_blocking(
        || (),
        move |_| {
            let v = initial_local_manuals.get_value();
            async move { Ok::<_, ServerFnError>(v) }
        },
    );
    let saved_videos_resource = Resource::new_blocking(
        || (),
        move |_| {
            let v = initial_saved_videos.get_value();
            async move { Ok::<_, ServerFnError>(v) }
        },
    );
    let saved_resource_links_resource = Resource::new_blocking(
        || (),
        move |_| {
            let v = initial_saved_resource_links.get_value();
            async move { Ok::<_, ServerFnError>(v) }
        },
    );
    let manual_suggestions_resource = Resource::new_blocking(
        || (),
        move |_| {
            let v = initial_manual_suggestions.get_value();
            async move { Ok::<_, ServerFnError>(v) }
        },
    );
    let video_suggestions_resource = Resource::new_blocking(
        || (),
        move |_| {
            let v = initial_video_suggestions.get_value();
            async move { Ok::<_, ServerFnError>(v) }
        },
    );

    let docs = move || docs_resource.get().and_then(Result::ok).unwrap_or_default();
    let local_manuals = move || {
        local_manuals_resource
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
    };
    let saved_videos = move || {
        saved_videos_resource
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
    };
    let saved_resource_links = move || {
        saved_resource_links_resource
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
    };
    let manual_suggestions = move || {
        manual_suggestions_resource
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
    };
    let video_suggestions = move || {
        video_suggestions_resource
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
    };

    let show_all_suggestions = RwSignal::new(false);
    let hidden_manual_ids = RwSignal::new(HashSet::<String>::new());
    let saved_manual_source_urls = RwSignal::new(HashSet::<String>::new());
    let added_manual_link_items = RwSignal::new(Vec::<ResourceLink>::new());
    let hidden_resource_link_ids = RwSignal::new(HashSet::<String>::new());
    let saved_resource_link_urls = RwSignal::new(HashSet::<String>::new());
    let added_resource_link_items = RwSignal::new(Vec::<ResourceLink>::new());
    let removed_video_ids = RwSignal::new(HashSet::<String>::new());
    let pinned_video_ids = RwSignal::new(HashSet::<String>::new());
    let added_video_items = RwSignal::new(Vec::<ResourceVideo>::new());
    let local_manuals_cache = RwSignal::new(Vec::<LocalManual>::new());
    let saved_videos_cache = RwSignal::new(Vec::<VideoEntry>::new());
    let saved_resource_links_cache = RwSignal::new(Vec::<ResourceEntry>::new());
    let pending_link_actions = RwSignal::new(HashSet::<String>::new());
    let pending_video_actions = RwSignal::new(HashSet::<String>::new());
    let add_resource_url = RwSignal::new(String::new());
    let adding_resource = RwSignal::new(false);
    let upload_input_ref = NodeRef::<html::Input>::new();
    let uploading_manual = RwSignal::new(false);
    let resource_status = RwSignal::new(Option::<(ResourceStatusKind, String)>::None);

    Effect::new(move |_| {
        if let Some(Ok(manuals)) = local_manuals_resource.get() {
            local_manuals_cache.set(manuals);
        }
    });
    Effect::new(move |_| {
        if let Some(Ok(videos)) = saved_videos_resource.get() {
            saved_videos_cache.set(videos);
        }
    });
    Effect::new(move |_| {
        if let Some(Ok(links)) = saved_resource_links_resource.get() {
            saved_resource_links_cache.set(links);
        }
    });

    let visible_manuals = move || {
        let hidden = hidden_manual_ids.get();
        local_manuals()
            .into_iter()
            .filter(|manual| {
                manual
                    .delete_id
                    .as_ref()
                    .is_none_or(|delete_id| !hidden.contains(delete_id))
            })
            .collect::<Vec<_>>()
    };
    let visible_saved_videos = move || {
        let removed = removed_video_ids.get();
        saved_videos()
            .into_iter()
            .filter(|video| !removed.contains(&video.id))
            .collect::<Vec<_>>()
    };

    let saved_links = move || {
        let hidden_link_ids = hidden_resource_link_ids.get();
        let mut seen = HashSet::new();
        let mut links = Vec::new();
        for link in added_manual_link_items.get() {
            if seen.insert(resource_link_dedupe_key(&link)) {
                links.push(link);
            }
        }
        for link in added_resource_link_items.get() {
            if seen.insert(resource_link_dedupe_key(&link)) {
                links.push(link);
            }
        }
        for entry in saved_resource_links() {
            if hidden_link_ids.contains(&entry.id) {
                continue;
            }
            let link = saved_resource_link(entry);
            if seen.insert(resource_link_dedupe_key(&link)) {
                links.push(link);
            }
        }
        for doc in docs() {
            let link = document_resource_link(doc, system.get_value(), rom_filename.get_value());
            if seen.insert(resource_link_dedupe_key(&link)) {
                links.push(link);
            }
        }
        for manual in visible_manuals() {
            let link = local_manual_resource_link(manual);
            if seen.insert(resource_link_dedupe_key(&link)) {
                links.push(link);
            }
        }
        links
    };
    let saved_video_items = move || {
        let removed = removed_video_ids.get();
        let mut seen = HashSet::new();
        let mut items = Vec::new();
        for item in added_video_items.get() {
            if resource_video_is_removed(&item, &removed) {
                continue;
            }
            if seen.insert(resource_video_dedupe_key(&item)) {
                items.push(item);
            }
        }
        for item in visible_saved_videos().into_iter().map(saved_video_resource) {
            if seen.insert(resource_video_dedupe_key(&item)) {
                items.push(item);
            }
        }
        items
    };
    let saved_has_any = move || !saved_links().is_empty() || !saved_video_items().is_empty();
    let saved_resource_urls = move || {
        let mut urls = saved_manual_source_urls.get();
        urls.extend(saved_resource_link_urls.get());
        let hidden_link_ids = hidden_resource_link_ids.get();
        for entry in saved_resource_links() {
            if hidden_link_ids.contains(&entry.id) {
                continue;
            }
            urls.insert(entry.url);
        }
        for manual in visible_manuals() {
            urls.insert(manual.url);
            if let Some(source_url) = manual.source_url {
                urls.insert(source_url);
            }
        }
        urls
    };
    let saved_video_resource_ids = move || {
        let mut ids = pinned_video_ids.get();
        for video in visible_saved_videos() {
            ids.insert(video.url);
            ids.insert(video.video_id);
        }
        ids
    };
    let visible_manuals_untracked = move || {
        let hidden = hidden_manual_ids.get_untracked();
        local_manuals_cache
            .get_untracked()
            .into_iter()
            .filter(|manual| {
                manual
                    .delete_id
                    .as_ref()
                    .is_none_or(|delete_id| !hidden.contains(delete_id))
            })
            .collect::<Vec<_>>()
    };
    let visible_saved_videos_untracked = move || {
        let removed = removed_video_ids.get_untracked();
        saved_videos_cache
            .get_untracked()
            .into_iter()
            .filter(|video| !removed.contains(&video.id))
            .collect::<Vec<_>>()
    };
    let saved_resource_urls_untracked = move || {
        let mut urls = saved_manual_source_urls.get_untracked();
        urls.extend(saved_resource_link_urls.get_untracked());
        let hidden_link_ids = hidden_resource_link_ids.get_untracked();
        for entry in saved_resource_links_cache.get_untracked() {
            if hidden_link_ids.contains(&entry.id) {
                continue;
            }
            urls.insert(entry.url);
        }
        for manual in visible_manuals_untracked() {
            urls.insert(manual.url);
            if let Some(source_url) = manual.source_url {
                urls.insert(source_url);
            }
        }
        urls
    };
    let saved_video_resource_ids_untracked = move || {
        let mut ids = pinned_video_ids.get_untracked();
        for video in visible_saved_videos_untracked() {
            ids.insert(video.url);
            ids.insert(video.video_id);
        }
        ids
    };

    let suggested_links = move || {
        let saved_urls = saved_resource_urls();
        let mut seen = HashSet::new();
        let mut links = Vec::new();
        for rec in manual_suggestions() {
            if !saved_urls.contains(&rec.url) {
                let link = manual_suggestion_link(rec);
                if seen.insert(resource_link_dedupe_key(&link)) {
                    links.push(link);
                }
            }
        }
        for row in library_resources.get_value() {
            if row.resource_type != "video" && !saved_urls.contains(&row.url) {
                let link = library_resource_link(row);
                if seen.insert(resource_link_dedupe_key(&link)) {
                    links.push(link);
                }
            }
        }
        links
    };
    let suggested_video_items = move || {
        let saved_video_ids = saved_video_resource_ids();
        let mut seen = HashSet::new();
        let mut videos = Vec::new();
        for video in video_suggestions() {
            if video_matches_saved(&video, &saved_video_ids) {
                continue;
            }
            let item = suggested_video_resource(video, "metadata".to_string());
            if seen.insert(resource_video_dedupe_key(&item)) {
                videos.push(item);
            }
        }
        for row in library_resources.get_value() {
            if row.resource_type == "video" && !resource_video_matches_saved(&row, &saved_video_ids)
            {
                let item =
                    suggested_video_resource(library_video_suggestion(row), "metadata".to_string());
                if seen.insert(resource_video_dedupe_key(&item)) {
                    videos.push(item);
                }
            }
        }
        videos
    };

    let suggested_resource_count = move || suggested_links().len() + suggested_video_items().len();
    let should_collapse_suggestions = move || {
        saved_has_any()
            && !show_all_suggestions.get()
            && (suggested_links().len() > COLLAPSED_SUGGESTION_LINK_COUNT
                || suggested_video_items().len() > COLLAPSED_SUGGESTION_VIDEO_COUNT)
    };
    let visible_suggestion_links = move || {
        let all = suggested_links();
        if !should_collapse_suggestions() {
            all
        } else {
            all.into_iter()
                .take(COLLAPSED_SUGGESTION_LINK_COUNT)
                .collect()
        }
    };
    let visible_suggestion_videos = move || {
        let all = suggested_video_items();
        if !should_collapse_suggestions() {
            return all;
        }
        all.into_iter()
            .take(COLLAPSED_SUGGESTION_VIDEO_COUNT)
            .collect::<Vec<_>>()
    };
    let hidden_suggestion_count = move || {
        suggested_resource_count()
            .saturating_sub(visible_suggestion_links().len() + visible_suggestion_videos().len())
    };
    let on_link_action = Callback::new(move |action: ResourceAction| match action {
        ResourceAction::SaveManual {
            url,
            language,
            title,
            source,
        } => {
            let pending_key = format!("save-manual:{}", normalized_url_key(&url));
            if pending_link_actions.read().contains(&pending_key) {
                return;
            }
            let saved_urls = saved_resource_urls_untracked();
            if saved_urls.contains(&url) || saved_urls.contains(&normalized_url_key(&url)) {
                set_resource_status(
                    resource_status,
                    ResourceStatusKind::Warning,
                    t(
                        i18n.locale.get_untracked(),
                        Key::GameDetailResourceAlreadySaved,
                    ),
                );
                return;
            }
            pending_link_actions.update(|keys| {
                keys.insert(pending_key.clone());
            });
            let sys = system.get_value();
            let fname = rom_filename.get_value();
            let bt = base_title.get_value();
            resource_status.set(None);
            leptos::task::spawn_local(async move {
                let saved_url = url.clone();
                match server_fns::download_manual(
                    sys.clone(),
                    fname,
                    bt.clone(),
                    url.clone(),
                    language,
                    Some(title),
                    Some(source),
                )
                .await
                {
                    Ok(_) => {
                        saved_manual_source_urls.update(|urls| {
                            urls.insert(saved_url);
                        });
                        refresh_added_manual_links(sys, bt, added_manual_link_items).await;
                        set_resource_status(
                            resource_status,
                            ResourceStatusKind::Success,
                            t(i18n.locale.get_untracked(), Key::GameDetailManualSaved),
                        );
                    }
                    Err(e) => {
                        set_server_error_status(resource_status, i18n.locale.get_untracked(), e);
                    }
                }
                pending_link_actions.update(|keys| {
                    keys.remove(&pending_key);
                });
            });
        }
        ResourceAction::SaveLink {
            url,
            title,
            source,
            resource_type,
        } => {
            let pending_key = format!("save-link:{}", normalized_url_key(&url));
            if pending_link_actions.read().contains(&pending_key) {
                return;
            }
            let saved_urls = saved_resource_urls_untracked();
            if saved_urls.contains(&url) || saved_urls.contains(&normalized_url_key(&url)) {
                set_resource_status(
                    resource_status,
                    ResourceStatusKind::Warning,
                    t(
                        i18n.locale.get_untracked(),
                        Key::GameDetailResourceAlreadySaved,
                    ),
                );
                return;
            }
            pending_link_actions.update(|keys| {
                keys.insert(pending_key.clone());
            });
            let sys = system.get_value();
            let fname = rom_filename.get_value();
            let bt = base_title.get_value();
            resource_status.set(None);
            leptos::task::spawn_local(async move {
                match server_fns::add_game_resource_link(
                    sys,
                    fname,
                    bt,
                    url.clone(),
                    title,
                    source,
                    resource_type,
                )
                .await
                {
                    Ok(entry) => {
                        added_resource_link_items.update(|items| {
                            let link = saved_resource_link(entry.clone());
                            if !items.iter().any(|existing| {
                                resource_link_dedupe_key(existing)
                                    == resource_link_dedupe_key(&link)
                            }) {
                                items.insert(0, link);
                            }
                        });
                        saved_resource_link_urls.update(|urls| {
                            urls.insert(entry.url);
                        });
                        set_resource_status(
                            resource_status,
                            ResourceStatusKind::Success,
                            t(i18n.locale.get_untracked(), Key::GameDetailResourceSaved),
                        );
                    }
                    Err(e) => {
                        set_server_error_status(resource_status, i18n.locale.get_untracked(), e);
                    }
                }
                pending_link_actions.update(|keys| {
                    keys.remove(&pending_key);
                });
            });
        }
        ResourceAction::RemoveManual {
            delete_id,
            href,
            source_url,
        } => {
            if !confirm_resource_delete() {
                return;
            }
            let pending_key = format!("remove-manual:{delete_id}");
            if pending_link_actions.read().contains(&pending_key) {
                return;
            }
            pending_link_actions.update(|keys| {
                keys.insert(pending_key.clone());
            });
            let sys = system.get_value();
            leptos::task::spawn_local(async move {
                match server_fns::delete_manual(sys, delete_id.clone()).await {
                    Ok(()) => {
                        hidden_manual_ids.update(|ids| {
                            ids.insert(delete_id.clone());
                        });
                        saved_manual_source_urls.update(|urls| {
                            urls.remove(&href);
                            urls.remove(&normalized_url_key(&href));
                            if let Some(source_url) = &source_url {
                                urls.remove(source_url);
                                urls.remove(&normalized_url_key(source_url));
                            }
                        });
                        added_manual_link_items.update(|items| {
                            items.retain(|item| match &item.action {
                                ResourceAction::RemoveManual {
                                    delete_id: item_id, ..
                                } => item_id != &delete_id,
                                _ => true,
                            });
                        });
                    }
                    Err(e) => {
                        set_server_error_status(resource_status, i18n.locale.get_untracked(), e)
                    }
                }
                pending_link_actions.update(|keys| {
                    keys.remove(&pending_key);
                });
            });
        }
        ResourceAction::RemoveLink {
            id,
            rom_filename,
            href,
        } => {
            if !confirm_resource_delete() {
                return;
            }
            let pending_key = format!("remove-link:{rom_filename}:{id}");
            if pending_link_actions.read().contains(&pending_key) {
                return;
            }
            pending_link_actions.update(|keys| {
                keys.insert(pending_key.clone());
            });
            let sys = system.get_value();
            leptos::task::spawn_local(async move {
                match server_fns::remove_game_resource_link(sys, rom_filename, id.clone()).await {
                    Ok(()) => {
                        hidden_resource_link_ids.update(|ids| {
                            ids.insert(id.clone());
                        });
                        saved_resource_link_urls.update(|urls| {
                            urls.remove(&href);
                            urls.remove(&normalized_url_key(&href));
                        });
                        added_resource_link_items.update(|items| {
                            items.retain(|item| match &item.action {
                                ResourceAction::RemoveLink { id: item_id, .. } => item_id != &id,
                                _ => true,
                            });
                        });
                    }
                    Err(e) => {
                        set_server_error_status(resource_status, i18n.locale.get_untracked(), e)
                    }
                }
                pending_link_actions.update(|keys| {
                    keys.remove(&pending_key);
                });
            });
        }
        ResourceAction::None => {}
    });
    let on_video_action = Callback::new(move |action: VideoResourceAction| match action {
        VideoResourceAction::Pin { video, tag } => {
            let pending_key = format!("pin-video:{}", video_identity(&video.url));
            if pending_video_actions.read().contains(&pending_key) {
                return;
            }
            if video_url_matches_saved(&video.url, &saved_video_resource_ids_untracked()) {
                set_resource_status(
                    resource_status,
                    ResourceStatusKind::Warning,
                    t(
                        i18n.locale.get_untracked(),
                        Key::GameDetailResourceAlreadySaved,
                    ),
                );
                return;
            }
            pending_video_actions.update(|keys| {
                keys.insert(pending_key.clone());
            });
            let sys = system.get_value();
            let fname = rom_filename.get_value();
            let bt = base_title.get_value();
            let url = video.url.clone();
            let title = Some(video.title.clone());
            leptos::task::spawn_local(async move {
                match server_fns::add_game_video(sys, fname, bt, url, title, true, Some(tag)).await
                {
                    Ok(entry) => {
                        removed_video_ids.update(|ids| {
                            ids.remove(&entry.id);
                        });
                        added_video_items.update(|items| {
                            let item = saved_video_resource(entry.clone());
                            if !items.iter().any(|existing| {
                                resource_video_dedupe_key(existing)
                                    == resource_video_dedupe_key(&item)
                            }) {
                                items.insert(0, item);
                            }
                        });
                        pinned_video_ids.update(|ids| {
                            ids.insert(entry.url);
                            ids.insert(entry.video_id);
                        });
                        set_resource_status(
                            resource_status,
                            ResourceStatusKind::Success,
                            t(i18n.locale.get_untracked(), Key::GameDetailResourceSaved),
                        );
                    }
                    Err(e) => {
                        set_server_error_status(resource_status, i18n.locale.get_untracked(), e)
                    }
                }
                pending_video_actions.update(|keys| {
                    keys.remove(&pending_key);
                });
            });
        }
        VideoResourceAction::Remove {
            id,
            rom_filename,
            href,
        } => {
            if !confirm_resource_delete() {
                return;
            }
            let pending_key = format!("remove-video:{rom_filename}:{id}");
            if pending_video_actions.read().contains(&pending_key) {
                return;
            }
            pending_video_actions.update(|keys| {
                keys.insert(pending_key.clone());
            });
            let sys = system.get_value();
            leptos::task::spawn_local(async move {
                match server_fns::remove_game_video(sys, rom_filename, id.clone()).await {
                    Ok(()) => {
                        removed_video_ids.update(|ids| {
                            ids.insert(id.clone());
                        });
                        pinned_video_ids.update(|ids| {
                            remove_saved_video_markers(ids, &id, &href);
                        });
                        added_video_items.update(|items| {
                            items.retain(|item| match &item.action {
                                VideoResourceAction::Remove { id: item_id, .. } => item_id != &id,
                                VideoResourceAction::Pin { .. } => true,
                            });
                        });
                    }
                    Err(e) => {
                        set_server_error_status(resource_status, i18n.locale.get_untracked(), e)
                    }
                }
                pending_video_actions.update(|keys| {
                    keys.remove(&pending_key);
                });
            });
        }
    });
    // Online video search: one results panel at a time. Picking a new kind
    // replaces the panel rather than stacking results below earlier searches.
    let active_search = RwSignal::new(Option::<VideoSearchKind>::None);
    let active_results = RwSignal::new(Vec::<VideoRecommendation>::new());
    let active_searching = RwSignal::new(false);
    let active_error = RwSignal::new(false);
    let start_search = move |kind: VideoSearchKind| {
        active_search.set(Some(kind));
        active_searching.set(true);
        active_error.set(false);
        active_results.set(vec![]);
        let sys = system.get_value();
        let dn = display_name.get_value();
        leptos::task::spawn_local(async move {
            match server_fns::search_game_videos(sys, dn, kind.query_type().to_string()).await {
                Ok(results) => active_results.set(results),
                Err(_) => active_error.set(true),
            }
            active_searching.set(false);
        });
    };
    // Search results as pinnable rows, hiding any already pinned for this game.
    let search_result_videos = move || {
        let saved = saved_video_resource_ids();
        let tag = active_search
            .get()
            .map(|kind| kind.query_type().to_string())
            .unwrap_or_else(|| "search".to_string());
        active_results
            .get()
            .into_iter()
            .filter(|rec| !video_matches_saved(rec, &saved))
            .map(|rec| suggested_video_resource(rec, tag.clone()))
            .collect::<Vec<_>>()
    };

    let do_add_resource = move || {
        let url = add_resource_url.get().trim().to_string();
        if url.is_empty() || adding_resource.get_untracked() {
            return;
        }

        let sys = system.get_value();
        let fname = rom_filename.get_value();
        let bt = base_title.get_value();
        let is_video_url = video_url::parse_video_url(&url).is_ok();
        let is_manual_url = resource_url_points_to_manual(&url);
        if is_video_url && video_url_matches_saved(&url, &saved_video_resource_ids_untracked()) {
            set_resource_status(
                resource_status,
                ResourceStatusKind::Warning,
                t(
                    i18n.locale.get_untracked(),
                    Key::GameDetailResourceAlreadySaved,
                ),
            );
            return;
        }
        if !is_video_url {
            let saved_urls = saved_resource_urls_untracked();
            if saved_urls.contains(&url) || saved_urls.contains(&normalized_url_key(&url)) {
                set_resource_status(
                    resource_status,
                    ResourceStatusKind::Warning,
                    t(
                        i18n.locale.get_untracked(),
                        Key::GameDetailResourceAlreadySaved,
                    ),
                );
                return;
            }
        }
        adding_resource.set(true);
        resource_status.set(None);
        leptos::task::spawn_local(async move {
            if is_manual_url {
                match server_fns::download_manual(
                    sys.clone(),
                    fname,
                    bt.clone(),
                    url.clone(),
                    None,
                    Some(resource_title_from_url(&url)),
                    Some("user_url".to_string()),
                )
                .await
                {
                    Ok(_) => {
                        saved_manual_source_urls.update(|urls| {
                            urls.insert(url);
                        });
                        refresh_added_manual_links(sys, bt, added_manual_link_items).await;
                        add_resource_url.set(String::new());
                        set_resource_status(
                            resource_status,
                            ResourceStatusKind::Success,
                            t(i18n.locale.get_untracked(), Key::GameDetailManualSaved),
                        );
                    }
                    Err(e) => {
                        set_server_error_status(resource_status, i18n.locale.get_untracked(), e)
                    }
                }
            } else if is_video_url {
                match server_fns::add_game_video(
                    sys,
                    fname,
                    bt,
                    url,
                    None,
                    false,
                    Some("saved".to_string()),
                )
                .await
                {
                    Ok(entry) => {
                        removed_video_ids.update(|ids| {
                            ids.remove(&entry.id);
                        });
                        let item = saved_video_resource(entry.clone());
                        added_video_items.update(|items| {
                            if !items.iter().any(|existing| {
                                resource_video_dedupe_key(existing)
                                    == resource_video_dedupe_key(&item)
                            }) {
                                items.insert(0, item);
                            }
                        });
                        pinned_video_ids.update(|ids| {
                            ids.insert(entry.url);
                            ids.insert(entry.video_id);
                        });
                        add_resource_url.set(String::new());
                        set_resource_status(
                            resource_status,
                            ResourceStatusKind::Success,
                            t(i18n.locale.get_untracked(), Key::GameDetailResourceSaved),
                        );
                    }
                    Err(e) => {
                        set_server_error_status(resource_status, i18n.locale.get_untracked(), e)
                    }
                }
            } else {
                match server_fns::add_game_resource_link(
                    sys,
                    fname,
                    bt,
                    url.clone(),
                    resource_title_from_url(&url),
                    Some("saved".to_string()),
                    "link".to_string(),
                )
                .await
                {
                    Ok(entry) => {
                        let link = saved_resource_link(entry.clone());
                        added_resource_link_items.update(|items| {
                            if !items.iter().any(|existing| {
                                resource_link_dedupe_key(existing)
                                    == resource_link_dedupe_key(&link)
                            }) {
                                items.insert(0, link);
                            }
                        });
                        saved_resource_link_urls.update(|urls| {
                            urls.insert(entry.url);
                        });
                        add_resource_url.set(String::new());
                        set_resource_status(
                            resource_status,
                            ResourceStatusKind::Success,
                            t(i18n.locale.get_untracked(), Key::GameDetailResourceSaved),
                        );
                    }
                    Err(e) => {
                        set_server_error_status(resource_status, i18n.locale.get_untracked(), e)
                    }
                }
            }
            adding_resource.set(false);
        });
    };
    let on_upload_manual = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        resource_status.set(None);

        let sys = system.get_value();
        let fname = rom_filename.get_value();
        let bt = base_title.get_value();
        let title = base_title.get_value();

        #[cfg(target_arch = "wasm32")]
        {
            let choose_file_message =
                t(i18n.locale.get_untracked(), Key::GameDetailManualChooseFile);
            let Some(input) = upload_input_ref.get() else {
                set_resource_status(
                    resource_status,
                    ResourceStatusKind::Error,
                    choose_file_message,
                );
                return;
            };
            let Some(files) = input.files() else {
                set_resource_status(
                    resource_status,
                    ResourceStatusKind::Error,
                    choose_file_message,
                );
                return;
            };
            let Some(file) = files.item(0) else {
                set_resource_status(
                    resource_status,
                    ResourceStatusKind::Error,
                    choose_file_message,
                );
                return;
            };
            let filename = file.name();
            let lower = filename.to_lowercase();
            if !(lower.ends_with(".pdf") || lower.ends_with(".txt")) {
                set_resource_status(
                    resource_status,
                    ResourceStatusKind::Error,
                    t(
                        i18n.locale.get_untracked(),
                        Key::GameDetailManualInvalidFileType,
                    ),
                );
                return;
            }

            uploading_manual.set(true);
            leptos::task::spawn_local(async move {
                match upload_manual_file(&sys, &fname, &bt, &title, file).await {
                    Ok(()) => {
                        input.set_value("");
                        refresh_added_manual_links(sys, bt, added_manual_link_items).await;
                        set_resource_status(
                            resource_status,
                            ResourceStatusKind::Success,
                            t(i18n.locale.get_untracked(), Key::GameDetailManualSaved),
                        );
                    }
                    Err(e) => {
                        set_resource_status(resource_status, ResourceStatusKind::Error, e);
                    }
                }
                uploading_manual.set(false);
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (sys, fname, bt, title);
            set_resource_status(
                resource_status,
                ResourceStatusKind::Error,
                t(
                    i18n.locale.get_untracked(),
                    Key::GameDetailManualUploadBrowserOnly,
                ),
            );
        }
    };

    view! {
        <section node_ref=section_ref id=section_id class="section resources-section">
            <h2 class="section-title">{move || t(i18n.locale.get(), Key::GameDetailResources)}</h2>
            {move || {
                resource_status.get().map(|(kind, message)| {
                    view! {
                        <div class=resource_status_class(kind)>{message}</div>
                    }
                })
            }}

            <Suspense fallback=move || view! {
                <div class="resource-block">
                    <p class="game-section-empty">"Loading..."</p>
                </div>
            }>
                <Show when=move || !saved_has_any()>
                    <div class="resource-block">
                        <p class="game-section-empty">{move || t(i18n.locale.get(), Key::GameDetailNoSavedResources)}</p>
                    </div>
                </Show>

                <Show when=move || !saved_links().is_empty()>
                    <div class="resource-block">
                        <h3 class="resource-block-title">{move || t(i18n.locale.get(), Key::GameDetailManualsGuidesAndLinks)}</h3>
                        <div class="resource-link-grid">
                            <For each=saved_links key=resource_link_key let:link>
                                {
                                    let pending_key = resource_action_key(&link.action);
                                    let pending = Signal::derive(move || {
                                        pending_key
                                            .as_ref()
                                            .is_some_and(|key| pending_link_actions.read().contains(key))
                                    });
                                    view! { <ResourceLinkRow link=link on_action=on_link_action pending=pending /> }
                                }
                            </For>
                        </div>
                    </div>
                </Show>

                <Show when=move || !saved_video_items().is_empty()>
                    <div class="resource-block resource-video-block">
                        <h3 class="resource-block-title">{move || t(i18n.locale.get(), Key::GameDetailVideos)}</h3>
                        <div class="resource-video-list">
                            <For each=saved_video_items key=resource_video_key let:video>
                                {
                                    let pending_key = video_action_key(&video.action);
                                    let pending = Signal::derive(move || {
                                        pending_key
                                            .as_ref()
                                            .is_some_and(|key| pending_video_actions.read().contains(key))
                                    });
                                    view! { <ResourceVideoRow video=video on_action=on_video_action pending=pending /> }
                                }
                            </For>
                        </div>
                    </div>
                </Show>

                <Show when=move || { suggested_resource_count() > 0 }>
                    <div
                        class="resource-block suggested-resource-block"
                        class:suggested-resource-block-secondary=move || saved_has_any()
                    >
                        <h3 class="resource-block-title">{move || t(i18n.locale.get(), Key::GameDetailSuggestedResources)}</h3>
                        <Show when=move || !visible_suggestion_links().is_empty()>
                            <div class="resource-link-grid">
                                <For each=visible_suggestion_links key=resource_link_key let:link>
                                    {
                                        let pending_key = resource_action_key(&link.action);
                                        let pending = Signal::derive(move || {
                                            pending_key
                                                .as_ref()
                                                .is_some_and(|key| pending_link_actions.read().contains(key))
                                        });
                                        view! { <ResourceLinkRow link=link on_action=on_link_action pending=pending /> }
                                    }
                                </For>
                            </div>
                        </Show>
                        <Show when=move || !visible_suggestion_videos().is_empty()>
                            <div class="resource-video-list">
                                <For each=visible_suggestion_videos key=resource_video_key let:video>
                                    {
                                        let pending_key = video_action_key(&video.action);
                                        let pending = Signal::derive(move || {
                                            pending_key
                                                .as_ref()
                                                .is_some_and(|key| pending_video_actions.read().contains(key))
                                        });
                                        view! { <ResourceVideoRow video=video on_action=on_video_action pending=pending /> }
                                    }
                                </For>
                            </div>
                        </Show>
                        <Show when=should_collapse_suggestions>
                            <button
                                type="button"
                                class="game-action-btn resources-show-all"
                                on:click=move |_| show_all_suggestions.set(true)
                            >
                                {move || show_more_resources_label(i18n.locale.get(), hidden_suggestion_count())}
                            </button>
                        </Show>
                    </div>
                </Show>

                <div class="resource-block add-resource-block">
                    <h3 class="resource-block-title">{move || t(i18n.locale.get(), Key::GameDetailAddResource)}</h3>
                    <div class="resource-add-form">
                        <input
                            type="text"
                            class="form-input"
                            placeholder=move || t(i18n.locale.get(), Key::GameDetailResourceUrlPlaceholder)
                            prop:value=move || add_resource_url.get()
                            on:input=move |ev| add_resource_url.set(event_target_value(&ev))
                            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                if ev.key() == "Enter" {
                                    ev.prevent_default();
                                    do_add_resource();
                                }
                            }
                        />
                        <button
                            type="button"
                            class="game-action-btn"
                            prop:disabled=move || adding_resource.get() || add_resource_url.read().trim().is_empty()
                            on:click=move |_| do_add_resource()
                        >
                            {move || t(i18n.locale.get(), Key::GameDetailAddResourceSubmit)}
                        </button>
                    </div>
                    <div class="resource-upload-form">
                        <input node_ref=upload_input_ref type="file" class="manual-file-input" accept=".pdf,.txt,application/pdf,text/plain" />
                        <button
                            type="button"
                            class="game-action-btn"
                            prop:disabled=move || uploading_manual.get()
                            on:click=on_upload_manual
                        >
                            {move || {
                                if uploading_manual.get() {
                                    t(i18n.locale.get(), Key::GameDetailDownloading)
                                } else {
                                    t(i18n.locale.get(), Key::GameDetailUploadManual)
                                }
                            }}
                        </button>
                    </div>
                </div>

                <div class="resource-block video-search-block">
                    <h3 class="resource-block-title">{move || t(i18n.locale.get(), Key::GameDetailFindOnlineVideos)}</h3>
                    <div class="video-search-buttons">
                        {[VideoSearchKind::Trailer, VideoSearchKind::Gameplay, VideoSearchKind::OneCc]
                            .into_iter()
                            .map(|kind| view! {
                                <SearchKindButton
                                    kind
                                    active_search=active_search
                                    active_searching=active_searching
                                    on_click=start_search
                                />
                            })
                            .collect_view()}
                    </div>
                    <Show when=move || active_search.get().is_some()>
                        <Show when=move || active_error.get()>
                            <div class="status-msg status-err resource-status">{move || t(i18n.locale.get(), Key::GameDetailSearchError)}</div>
                        </Show>
                        <Show when=move || active_searching.get()>
                            <p class="game-section-empty">{move || t(i18n.locale.get(), Key::CommonSearching)}</p>
                        </Show>
                        <Show when=move || !active_searching.get() && !active_error.get() && search_result_videos().is_empty()>
                            <p class="game-section-empty">{move || t(i18n.locale.get(), Key::GameDetailNoResults)}</p>
                        </Show>
                        <Show when=move || !search_result_videos().is_empty()>
                            <div class="resource-video-list">
                                <For each=search_result_videos key=resource_video_key let:video>
                                    {
                                        let pending_key = video_action_key(&video.action);
                                        let pending = Signal::derive(move || {
                                            pending_key
                                                .as_ref()
                                                .is_some_and(|key| pending_video_actions.read().contains(key))
                                        });
                                        view! { <ResourceVideoRow video=video on_action=on_video_action pending=pending /> }
                                    }
                                </For>
                            </div>
                        </Show>
                    </Show>
                </div>
            </Suspense>
        </section>
    }
}

#[component]
fn SearchKindButton<F>(
    kind: VideoSearchKind,
    active_search: RwSignal<Option<VideoSearchKind>>,
    active_searching: RwSignal<bool>,
    on_click: F,
) -> impl IntoView
where
    F: Fn(VideoSearchKind) + Copy + Send + 'static,
{
    let i18n = use_i18n();
    view! {
        <button
            type="button"
            class="game-action-btn resource-search-btn"
            class:active=move || active_search.get() == Some(kind)
            prop:disabled=move || active_searching.get()
            on:click=move |_| on_click(kind)
        >
            {move || {
                if active_searching.get() && active_search.get() == Some(kind) {
                    t(i18n.locale.get(), Key::CommonSearching)
                } else {
                    t(i18n.locale.get(), kind.label_key())
                }
            }}
        </button>
    }
}

#[component]
fn ResourceLinkRow(
    link: ResourceLink,
    on_action: Callback<ResourceAction>,
    #[prop(into)] pending: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let href = StoredValue::new(link.href);
    let title = link.title;
    let meta = link.meta;
    let icon = link.icon;
    let action = link.action;
    let has_action = !matches!(action, ResourceAction::None);
    let button_action = action.clone();
    let label_action = action.clone();
    let icon_action = action;
    let action_label = Signal::derive(move || match &label_action {
        ResourceAction::None => String::new(),
        ResourceAction::SaveManual { .. } | ResourceAction::SaveLink { .. } => {
            t(i18n.locale.get(), Key::CommonSave).to_string()
        }
        ResourceAction::RemoveManual { .. } | ResourceAction::RemoveLink { .. } => {
            t(i18n.locale.get(), Key::CommonDelete).to_string()
        }
    });
    let action_icon = match icon_action {
        ResourceAction::SaveManual { .. } | ResourceAction::SaveLink { .. } => {
            ResourceActionIcon::Plus
        }
        ResourceAction::RemoveManual { .. } | ResourceAction::RemoveLink { .. } => {
            ResourceActionIcon::Minus
        }
        ResourceAction::None => ResourceActionIcon::None,
    };

    view! {
        <div class="resource-link-row">
            <span class="resource-link-icon">{icon}</span>
            <a class="resource-link-main" href=move || href.get_value() target="_blank" rel="noopener">
                <span class="resource-link-title">{title}</span>
                <span class="resource-link-meta">{meta}</span>
            </a>
            <button
                type="button"
                class="resource-link-action resource-link-action-btn"
                class:resource-link-action-hidden=move || !has_action
                class:resource-link-action-pending=move || pending.get()
                prop:disabled=move || !has_action || pending.get()
                attr:aria-hidden=move || if has_action { "false" } else { "true" }
                aria-label=action_label
                title=action_label
                on:click=move |_| on_action.run(button_action.clone())
            >
                <ResourceActionStateIcon icon=action_icon pending=pending />
            </button>
        </div>
    }
}

#[derive(Clone, Copy)]
enum ResourceActionIcon {
    None,
    Plus,
    Minus,
    Spinner,
}

#[component]
fn ResourceActionStateIcon(
    icon: ResourceActionIcon,
    #[prop(into)] pending: Signal<bool>,
) -> impl IntoView {
    view! {
        <Show
            when=move || pending.get()
            fallback=move || view! { <ResourceActionSvgIcon icon=icon /> }
        >
            <ResourceActionSvgIcon icon=ResourceActionIcon::Spinner />
        </Show>
    }
}

#[component]
fn ResourceActionSvgIcon(icon: ResourceActionIcon) -> impl IntoView {
    match icon {
        ResourceActionIcon::None => view! {
            <span class="resource-action-icon" aria-hidden="true"></span>
        }
        .into_any(),
        ResourceActionIcon::Plus => view! {
            <svg class="resource-action-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M12 5v14"></path>
                <path d="M5 12h14"></path>
            </svg>
        }
        .into_any(),
        ResourceActionIcon::Minus => view! {
            <svg class="resource-action-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M5 12h14"></path>
            </svg>
        }
        .into_any(),
        ResourceActionIcon::Spinner => view! {
            <svg class="resource-action-icon resource-action-spinner" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M21 12a9 9 0 1 1-3.2-6.9"></path>
                <path d="M21 3v6h-6"></path>
            </svg>
        }
        .into_any(),
    }
}

#[component]
fn ResourceVideoRow(
    video: ResourceVideo,
    on_action: Callback<VideoResourceAction>,
    #[prop(into)] pending: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let href = StoredValue::new(video.href);
    let title = video.title;
    let thumbnail_url = video.thumbnail_url;
    let embed_url = StoredValue::new(video.embed_url);
    let meta = StoredValue::new(video.meta);
    let action = video.action;
    let button_action = action.clone();
    let label_action = action.clone();
    let icon_action = action;
    let action_label = Signal::derive(move || match &label_action {
        VideoResourceAction::Pin { .. } => t(i18n.locale.get(), Key::GameDetailPinVideo),
        VideoResourceAction::Remove { .. } => t(i18n.locale.get(), Key::GameDetailRemoveVideo),
    });
    let action_icon = match icon_action {
        VideoResourceAction::Pin { .. } => ResourceActionIcon::Plus,
        VideoResourceAction::Remove { .. } => ResourceActionIcon::Minus,
    };
    let playing = RwSignal::new(false);
    let on_click_play = move |_| {
        if embed_url.get_value().is_some() {
            playing.update(|value| *value = !*value);
        }
    };

    view! {
        <div class="resource-video-item">
            <div
                class="resource-video-row"
                class:resource-video-row-expanded=move || playing.get()
            >
                <button
                    type="button"
                    class="resource-video-main"
                    title=move || href.get_value()
                    on:click=on_click_play
                >
                    <span class="recommendation-thumb-wrapper">
                        {thumbnail_url.map(|url| view! {
                            <img class="recommendation-thumb" src=url alt="" />
                        })}
                        <span class="recommendation-play-icon">"\u{25B6}"</span>
                    </span>
                    <span class="recommendation-info">
                        <span class="recommendation-title">{title}</span>
                        <Show when=move || !meta.get_value().is_empty()>
                            <span class="recommendation-meta">{meta.get_value()}</span>
                        </Show>
                    </span>
                </button>
                <button
                    type="button"
                    class="resource-link-action resource-link-action-btn"
                    class:resource-link-action-pending=move || pending.get()
                    prop:disabled=move || pending.get()
                    aria-label=action_label
                    title=action_label
                    on:click=move |_| on_action.run(button_action.clone())
                >
                    <ResourceActionStateIcon icon=action_icon pending=pending />
                </button>
            </div>
            <Show when=move || playing.get()>
                <div class="resource-video-player">
                    <div class="video-embed">
                        <iframe
                            src=move || embed_url.get_value().unwrap_or_default()
                            allowfullscreen=true
                            allow="autoplay; encrypted-media"
                            sandbox="allow-scripts allow-same-origin allow-popups"
                        ></iframe>
                    </div>
                </div>
            </Show>
        </div>
    }
}

fn document_resource_link(doc: GameDocument, system: String, rom_filename: String) -> ResourceLink {
    let icon = resource_icon(&doc.extension);
    let size = crate::util::format_size(doc.size_bytes);
    let ext = doc.extension.to_uppercase();
    let encoded_rom = crate::util::base64_encode(rom_filename.as_bytes());
    let encoded_path = urlencoding::encode(&doc.relative_path);
    ResourceLink {
        title: doc.label,
        meta: format!("{ext} · {size}"),
        href: format!("/rom-docs/{system}/{encoded_rom}/{encoded_path}"),
        icon,
        action: ResourceAction::None,
    }
}

fn local_manual_resource_link(manual: LocalManual) -> ResourceLink {
    let size = crate::util::format_size(manual.size_bytes);
    let kind = manual
        .filename
        .rsplit('.')
        .next()
        .unwrap_or("file")
        .to_uppercase();
    let mut meta = vec![kind, size];
    if let Some(provider) = manual.provider.as_deref().map(source_label) {
        meta.push(provider.to_string());
    }
    let action = manual
        .delete_id
        .map(|delete_id| ResourceAction::RemoveManual {
            delete_id,
            href: manual.url.clone(),
            source_url: manual.source_url.clone(),
        })
        .unwrap_or(ResourceAction::None);
    ResourceLink {
        title: manual.label,
        meta: meta.join(" · "),
        href: manual.url,
        icon: "\u{1F4C4}",
        action,
    }
}

fn manual_suggestion_link(rec: ManualRecommendation) -> ResourceLink {
    let mut meta = vec![rec.source.clone(), "Suggested manual".to_string()];
    if let Some(language) = &rec.language {
        meta.push(language.clone());
    }
    if let Some(size) = rec.size_bytes {
        meta.push(crate::util::format_size(size));
    }
    ResourceLink {
        title: rec.title.clone(),
        meta: meta.join(" · "),
        href: rec.url.clone(),
        icon: "\u{1F4C4}",
        action: ResourceAction::SaveManual {
            url: rec.url,
            language: rec.language,
            title: rec.title,
            source: rec.source,
        },
    }
}

fn library_resource_link(row: LibraryResourceLink) -> ResourceLink {
    let title = row
        .title
        .unwrap_or_else(|| source_label(&row.source).to_string());
    let meta = format!(
        "{} · {}",
        source_label(&row.source),
        resource_type_label(&row.resource_type)
    );
    ResourceLink {
        title: title.clone(),
        meta,
        href: row.url.clone(),
        icon: resource_type_icon(&row.resource_type),
        action: ResourceAction::SaveLink {
            url: row.url,
            title,
            source: Some(row.source),
            resource_type: row.resource_type,
        },
    }
}

fn saved_resource_link(entry: ResourceEntry) -> ResourceLink {
    let source = entry
        .source
        .as_deref()
        .map(source_label)
        .unwrap_or("Saved link");
    let meta = format!("{source} · {}", resource_type_label(&entry.resource_type));
    ResourceLink {
        title: entry.title,
        meta,
        href: entry.url.clone(),
        icon: resource_type_icon(&entry.resource_type),
        action: ResourceAction::RemoveLink {
            id: entry.id,
            rom_filename: entry.rom_filename,
            href: entry.url,
        },
    }
}

fn saved_video_resource(video: VideoEntry) -> ResourceVideo {
    let thumbnail_url = saved_video_thumbnail_url(&video);
    let embed_url = saved_video_embed_url(&video);
    let meta = video_meta_text([Some(video.platform.clone()), video.tag.clone()]);
    let href = video.url.clone();
    ResourceVideo {
        title: video.title.unwrap_or_else(|| video.url.clone()),
        meta,
        href,
        thumbnail_url,
        embed_url,
        action: VideoResourceAction::Remove {
            id: video.id,
            rom_filename: video.rom_filename,
            href: video.url,
        },
    }
}

fn suggested_video_resource(video: VideoRecommendation, tag: String) -> ResourceVideo {
    let meta = video_meta_text([video.channel.clone(), video.duration_text.clone()]);
    let embed_url = video_embed_url_from_url(&video.url).or_else(|| Some(video.url.clone()));
    ResourceVideo {
        title: video.title.clone(),
        meta,
        href: video.url.clone(),
        thumbnail_url: video.thumbnail_url.clone(),
        embed_url,
        action: VideoResourceAction::Pin { video, tag },
    }
}

fn library_video_suggestion(row: LibraryResourceLink) -> VideoRecommendation {
    VideoRecommendation {
        url: row.url,
        title: row
            .title
            .unwrap_or_else(|| source_label(&row.source).to_string()),
        thumbnail_url: None,
        duration_text: None,
        channel: Some(source_label(&row.source).to_string()),
    }
}

fn resource_link_key(link: &ResourceLink) -> String {
    format!("{}|{}|{}", link.href, link.title, link.meta)
}

fn resource_link_dedupe_key(link: &ResourceLink) -> String {
    normalized_url_key(&link.href)
}

fn resource_video_key(video: &ResourceVideo) -> String {
    format!("{}|{}", video.href, video.title)
}

fn resource_video_dedupe_key(video: &ResourceVideo) -> String {
    video_identity(&video.href)
}

fn resource_action_key(action: &ResourceAction) -> Option<String> {
    match action {
        ResourceAction::None => None,
        ResourceAction::SaveManual { url, .. } => {
            Some(format!("save-manual:{}", normalized_url_key(url)))
        }
        ResourceAction::SaveLink { url, .. } => {
            Some(format!("save-link:{}", normalized_url_key(url)))
        }
        ResourceAction::RemoveManual { delete_id, .. } => {
            Some(format!("remove-manual:{delete_id}"))
        }
        ResourceAction::RemoveLink {
            id, rom_filename, ..
        } => Some(format!("remove-link:{rom_filename}:{id}")),
    }
}

fn video_action_key(action: &VideoResourceAction) -> Option<String> {
    match action {
        VideoResourceAction::Pin { video, .. } => {
            Some(format!("pin-video:{}", video_identity(&video.url)))
        }
        VideoResourceAction::Remove {
            id, rom_filename, ..
        } => Some(format!("remove-video:{rom_filename}:{id}")),
    }
}

fn remove_saved_video_markers(ids: &mut HashSet<String>, id: &str, href: &str) {
    ids.remove(id);
    ids.remove(href);
    ids.remove(&video_identity(href));
    if let Some((_, platform_id)) = id.split_once('-') {
        ids.remove(platform_id);
    }
    if let Ok(parsed) = video_url::parse_video_url(href) {
        ids.remove(&parsed.video_id);
        ids.remove(&parsed.canonical_url);
    }
}

fn normalized_url_key(url: &str) -> String {
    url.trim_end_matches('/').to_ascii_lowercase()
}

fn resource_video_is_removed(video: &ResourceVideo, removed_ids: &HashSet<String>) -> bool {
    match &video.action {
        VideoResourceAction::Remove { id, .. } => removed_ids.contains(id),
        VideoResourceAction::Pin { .. } => false,
    }
}

fn video_matches_saved(video: &VideoRecommendation, saved_ids: &HashSet<String>) -> bool {
    let identity = video_identity(&video.url);
    saved_ids.contains(&video.url)
        || saved_ids.contains(&identity)
        || saved_ids
            .iter()
            .any(|id| !id.is_empty() && (video.url.contains(id) || identity == video_identity(id)))
}

fn resource_video_matches_saved(row: &LibraryResourceLink, saved_ids: &HashSet<String>) -> bool {
    let identity = video_identity(&row.url);
    saved_ids.contains(&row.url)
        || saved_ids.contains(&identity)
        || saved_ids.contains(&row.resource_id)
        || saved_ids
            .iter()
            .any(|id| !id.is_empty() && (row.url.contains(id) || identity == video_identity(id)))
}

fn video_url_matches_saved(url: &str, saved_ids: &HashSet<String>) -> bool {
    let identity = video_identity(url);
    saved_ids.contains(url)
        || saved_ids.contains(&identity)
        || saved_ids
            .iter()
            .any(|id| !id.is_empty() && (url.contains(id) || identity == video_identity(id)))
}

fn set_resource_status(
    status: RwSignal<Option<(ResourceStatusKind, String)>>,
    kind: ResourceStatusKind,
    message: impl Into<String>,
) {
    let message = message.into();
    status.set(Some((kind, message.clone())));

    #[cfg(target_arch = "wasm32")]
    if kind == ResourceStatusKind::Success {
        gloo_timers::callback::Timeout::new(RESOURCE_SUCCESS_VISIBLE_MS, move || {
            status.update(|current| {
                if matches!(current, Some((ResourceStatusKind::Success, current_message)) if current_message == &message)
                {
                    *current = None;
                }
            });
        })
        .forget();
    }
}

fn set_server_error_status(
    status: RwSignal<Option<(ResourceStatusKind, String)>>,
    locale: Locale,
    error: impl ToString,
) {
    let message = error.to_string();
    if message.contains("already saved") {
        set_resource_status(
            status,
            ResourceStatusKind::Warning,
            t(locale, Key::GameDetailResourceAlreadySaved),
        );
    } else {
        set_resource_status(status, ResourceStatusKind::Error, message);
    }
}

fn resource_status_class(kind: ResourceStatusKind) -> &'static str {
    match kind {
        ResourceStatusKind::Success => "status-msg status-ok resource-status",
        ResourceStatusKind::Warning => "status-msg status-warn resource-status",
        ResourceStatusKind::Error => "status-msg status-err resource-status",
    }
}

async fn refresh_added_manual_links(
    system: String,
    base_title: String,
    target: RwSignal<Vec<ResourceLink>>,
) {
    if let Ok(manuals) = server_fns::get_local_manuals(system, base_title).await {
        target.set(
            manuals
                .into_iter()
                .map(local_manual_resource_link)
                .collect(),
        );
    }
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

fn saved_video_thumbnail_url(video: &VideoEntry) -> Option<String> {
    (video.platform == "youtube" && !video.video_id.is_empty())
        .then(|| format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", video.video_id))
}

fn saved_video_embed_url(video: &VideoEntry) -> Option<String> {
    match video.platform.as_str() {
        "youtube" => Some(format!(
            "https://www.youtube-nocookie.com/embed/{}",
            video.video_id
        )),
        "twitch" => Some(format!(
            "https://player.twitch.tv/?video={}&parent=localhost",
            video.video_id
        )),
        "vimeo" => Some(format!("https://player.vimeo.com/video/{}", video.video_id)),
        "dailymotion" => Some(format!(
            "https://www.dailymotion.com/embed/video/{}",
            video.video_id
        )),
        _ => Some(video.url.clone()),
    }
}

fn video_embed_url_from_url(url: &str) -> Option<String> {
    youtube_id_from_url(url).map(|id| format!("https://www.youtube-nocookie.com/embed/{id}"))
}

fn video_meta_text(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{00B7} ")
}

fn show_more_resources_label(locale: Locale, count: usize) -> String {
    match locale {
        Locale::Es => format!("Ver {count} recursos más"),
        Locale::Ja => format!("残り{count}件のリソースを表示"),
        _ if count == 1 => "Show 1 more resource".to_string(),
        _ => format!("Show {count} more resources"),
    }
}

fn resource_url_points_to_manual(url: &str) -> bool {
    let path = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .trim()
        .to_ascii_lowercase();
    path.ends_with(".pdf") || path.ends_with(".txt")
}

fn confirm_resource_delete() -> bool {
    #[cfg(feature = "hydrate")]
    {
        web_sys::window()
            .and_then(|window| {
                window
                    .confirm_with_message("Remove this saved resource?")
                    .ok()
            })
            .unwrap_or(false)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        true
    }
}

fn video_identity(url: &str) -> String {
    if let Some(id) = youtube_id_from_url(url) {
        format!("youtube:{id}")
    } else {
        url.trim_end_matches('/').to_ascii_lowercase()
    }
}

fn youtube_id_from_url(url: &str) -> Option<String> {
    if let Some(id) = url.split("youtu.be/").nth(1) {
        return Some(
            id.split(['?', '&', '/', '#'])
                .next()
                .unwrap_or(id)
                .to_string(),
        );
    }
    if let Some(id) = url.split("youtube.com/embed/").nth(1) {
        return Some(
            id.split(['?', '&', '/', '#'])
                .next()
                .unwrap_or(id)
                .to_string(),
        );
    }
    url.split("v=").nth(1).map(|id| {
        id.split(['?', '&', '/', '#'])
            .next()
            .unwrap_or(id)
            .to_string()
    })
}

fn resource_icon(extension: &str) -> &'static str {
    match extension {
        "pdf" => "\u{1F4C4}",
        "txt" => "\u{1F4DD}",
        "jpg" | "jpeg" | "png" | "gif" => "\u{1F5BC}",
        "html" | "htm" => "\u{1F310}",
        _ => "\u{1F4CE}",
    }
}

fn resource_type_icon(resource_type: &str) -> &'static str {
    match resource_type {
        "video" => "\u{25B6}",
        "manual" => "\u{1F4C4}",
        "strategy_guide" | "video_index" => "\u{1F517}",
        _ => "\u{1F517}",
    }
}

fn source_label(source: &str) -> &str {
    match source {
        "shmups_wiki" => "Shmups Wiki",
        "mister_manuals" => "MiSTer Manuals",
        "retrokit" => "Retrokit",
        "launchbox" => "LaunchBox",
        "user_upload" => "User upload",
        "user_url" => "User URL",
        "saved" => "Saved",
        _ => source,
    }
}

fn resource_type_label(resource_type: &str) -> &str {
    match resource_type {
        "video" => "Video",
        "manual" => "Manual",
        "strategy_guide" => "Strategy guide",
        "video_index" => "Video index",
        _ => "Link",
    }
}

fn resource_title_from_url(url: &str) -> String {
    let without_scheme = url
        .trim()
        .strip_prefix("https://")
        .or_else(|| url.trim().strip_prefix("http://"))
        .unwrap_or_else(|| url.trim());
    without_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(without_scheme)
        .trim_end_matches('/')
        .to_string()
}
