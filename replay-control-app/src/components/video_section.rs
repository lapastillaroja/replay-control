use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, VideoEntry, VideoRecommendation};

/// Maximum number of embedded videos shown before "Show all".
const INITIAL_VIDEO_COUNT: usize = 3;

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

/// Full video section: saved videos, add input, search buttons, and results.
#[component]
pub fn GameVideoSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    display_name: StoredValue<String>,
    base_title: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    // Saved videos signal — starts from SSR resource, then updated locally.
    let saved_videos = RwSignal::new(Vec::<VideoEntry>::new());
    let show_all = RwSignal::new(false);

    // Load saved videos on mount — queries by base_title for cross-variant sharing.
    let videos_resource = Resource::new(
        move || (system.get_value(), base_title.get_value()),
        |(sys, bt)| server_fns::get_game_videos(sys, bt),
    );

    // Sync resource into signal when it resolves.
    let _sync = Effect::new(move || {
        if let Some(Ok(vids)) = videos_resource.get() {
            saved_videos.set(vids);
        }
    });

    let provider_results = RwSignal::new(Vec::<VideoRecommendation>::new());
    let provider_searching = RwSignal::new(false);
    let provider_error = RwSignal::new(false);
    let provider_loaded = RwSignal::new(false);
    let provider_resource = Resource::new(
        || (),
        move |_| {
            let sys = system.get_value();
            let fname = rom_filename.get_value();
            server_fns::get_provider_game_videos(sys, fname)
        },
    );
    let _sync_provider = Effect::new(move || {
        if let Some(result) = provider_resource.get() {
            provider_loaded.set(true);
            match result {
                Ok(results) => provider_results.set(results),
                Err(_) => provider_error.set(true),
            }
        }
    });

    // Add video state
    let add_url = RwSignal::new(String::new());
    let add_error = RwSignal::new(Option::<Key>::None);
    let add_success = RwSignal::new(false);
    let adding = RwSignal::new(false);

    let do_add_video = move || {
        let url = add_url.get();
        if url.trim().is_empty() {
            return;
        }
        adding.set(true);
        add_error.set(None);
        add_success.set(false);

        let sys = system.get_value();
        let fname = rom_filename.get_value();
        let bt = base_title.get_value();
        leptos::task::spawn_local(async move {
            match server_fns::add_game_video(sys, fname, bt, url, None, false, None).await {
                Ok(entry) => {
                    saved_videos.update(|vids| vids.insert(0, entry));
                    add_url.set(String::new());
                    add_success.set(true);
                    add_error.set(None);
                }
                Err(e) => {
                    let msg = e.to_string();
                    // Detect duplicate error
                    if msg.contains("already saved") {
                        add_error.set(Some(Key::GameDetailAddVideoDuplicate));
                    } else {
                        add_error.set(Some(Key::GameDetailAddVideoError));
                    }
                    add_success.set(false);
                }
            }
            adding.set(false);
        });
    };

    // Remove video handler — uses the video's own rom_filename (from the DB row).
    let on_remove = move |video_id: String, video_rom_filename: String| {
        let sys = system.get_value();
        let vid = video_id.clone();
        saved_videos.update(|vids| vids.retain(|v| v.id != vid));
        leptos::task::spawn_local(async move {
            let _ = server_fns::remove_game_video(sys, video_rom_filename, video_id).await;
        });
    };

    // Search state: one visible results panel. Replacing the panel avoids hiding
    // later searches below earlier result blocks.
    let active_search = RwSignal::new(Option::<VideoSearchKind>::None);
    let active_results = RwSignal::new(Vec::<VideoRecommendation>::new());
    let active_searching = RwSignal::new(false);
    let active_error = RwSignal::new(false);
    let active_tag = Signal::derive(move || {
        active_search
            .get()
            .map(|kind| kind.query_type())
            .unwrap_or("video")
            .to_string()
    });

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

    // Pin handler — adds a recommendation to saved videos
    let pin_video = move |rec: VideoRecommendation, tag: String| {
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        let bt = base_title.get_value();
        let url = rec.url.clone();
        let title = Some(rec.title.clone());
        leptos::task::spawn_local(async move {
            if let Ok(entry) =
                server_fns::add_game_video(sys, fname, bt, url, title, true, Some(tag)).await
            {
                saved_videos.update(|vids| vids.insert(0, entry));
            }
        });
    };

    let has_videos = move || !saved_videos.read().is_empty();
    let visible_videos = move || {
        let vids = saved_videos.get();
        if show_all.get() || vids.len() <= INITIAL_VIDEO_COUNT {
            vids
        } else {
            vids[..INITIAL_VIDEO_COUNT].to_vec()
        }
    };
    let has_more = move || saved_videos.read().len() > INITIAL_VIDEO_COUNT && !show_all.get();
    let visible_provider_results = Signal::derive(move || {
        visible_recommendations(&provider_results.read(), &saved_videos.read())
    });
    let has_provider_results = move || !visible_provider_results.read().is_empty();
    let show_empty_state =
        move || !has_videos() && provider_loaded.get() && provider_results.read().is_empty();

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameDetailVideos)}</h2>

            // Saved videos list
            <Show when=has_videos>
                <div class="video-list">
                    <For
                        each=visible_videos
                        key=|v| v.id.clone()
                        let:video
                    >
                        <VideoEmbed video=video.clone() on_remove=on_remove />
                    </For>
                    <Show when=has_more>
                        <button
                            class="game-action-btn"
                            style="margin-top: 4px"
                            on:click=move |_| show_all.set(true)
                        >
                            {move || t(i18n.locale.get(), Key::GameDetailShowAllVideos)}
                            {move || format!(" ({})", saved_videos.read().len())}
                        </button>
                    </Show>
                </div>
            </Show>
            <Show when=show_empty_state>
                <p class="game-section-empty">{move || t(i18n.locale.get(), Key::GameDetailNoVideos)}</p>
            </Show>

            <Show when=has_provider_results>
                <div class="video-subsection">
                    <Show when=has_videos>
                        <h3 class="video-subsection-title">
                            {move || t(i18n.locale.get(), Key::GameDetailSuggestedVideos)}
                        </h3>
                    </Show>
                    <VideoRecommendations
                        results=visible_provider_results
                        is_searching=provider_searching
                        has_error=provider_error
                        tag="metadata".to_string()
                        saved_videos=saved_videos
                        on_pin=pin_video
                    />
                </div>
            </Show>

            // Add video input
            <h3 class="video-subsection-title">{move || t(i18n.locale.get(), Key::GameDetailAddVideoUrl)}</h3>
            <div class="video-add-form">
                <input
                    type="text"
                    class="form-input"
                    placeholder=move || t(i18n.locale.get(), Key::GameDetailAddVideoPlaceholder)
                    prop:value=move || add_url.get()
                    on:input=move |ev| {
                        add_url.set(event_target_value(&ev));
                        add_error.set(None);
                        add_success.set(false);
                    }
                    on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                        if ev.key() == "Enter" {
                            do_add_video();
                        }
                    }
                />
                <button
                    class="game-action-btn"
                    prop:disabled=move || adding.get() || add_url.read().trim().is_empty()
                    on:click=move |_| do_add_video()
                >
                    {move || t(i18n.locale.get(), Key::GameDetailAddVideo)}
                </button>
            </div>
            <Show when=move || add_error.get().is_some()>
                <p class="video-add-error">{move || add_error.get().map(|k| t(i18n.locale.get(), k)).unwrap_or("")}</p>
            </Show>
            <Show when=move || add_success.get()>
                <p class="video-add-success">{move || t(i18n.locale.get(), Key::GameDetailVideoAdded)}</p>
            </Show>

            // Search buttons
            <h3 class="video-subsection-title">{move || t(i18n.locale.get(), Key::GameDetailFindOnlineVideos)}</h3>
            <div class="video-search-buttons">
                <button
                    class="game-action-btn"
                    class:active=move || active_search.get() == Some(VideoSearchKind::Trailer)
                    prop:disabled=move || active_searching.get()
                    on:click=move |_| start_search(VideoSearchKind::Trailer)
                >
                    {move || {
                        if active_searching.get() && active_search.get() == Some(VideoSearchKind::Trailer) {
                            t(i18n.locale.get(), Key::CommonSearching)
                        } else {
                            t(i18n.locale.get(), VideoSearchKind::Trailer.label_key())
                        }
                    }}
                </button>
                <button
                    class="game-action-btn"
                    class:active=move || active_search.get() == Some(VideoSearchKind::Gameplay)
                    prop:disabled=move || active_searching.get()
                    on:click=move |_| start_search(VideoSearchKind::Gameplay)
                >
                    {move || {
                        if active_searching.get() && active_search.get() == Some(VideoSearchKind::Gameplay) {
                            t(i18n.locale.get(), Key::CommonSearching)
                        } else {
                            t(i18n.locale.get(), VideoSearchKind::Gameplay.label_key())
                        }
                    }}
                </button>
                <button
                    class="game-action-btn"
                    class:active=move || active_search.get() == Some(VideoSearchKind::OneCc)
                    prop:disabled=move || active_searching.get()
                    on:click=move |_| start_search(VideoSearchKind::OneCc)
                >
                    {move || {
                        if active_searching.get() && active_search.get() == Some(VideoSearchKind::OneCc) {
                            t(i18n.locale.get(), Key::CommonSearching)
                        } else {
                            t(i18n.locale.get(), VideoSearchKind::OneCc.label_key())
                        }
                    }}
                </button>
            </div>

            // Active online search results
            <Show when=move || active_search.get().is_some()>
                <VideoRecommendations
                    results=Signal::derive(move || {
                        visible_recommendations(&active_results.read(), &saved_videos.read())
                    })
                    is_searching=active_searching
                    has_error=active_error
                    tag=active_tag
                    saved_videos=saved_videos
                    on_pin=pin_video
                />
            </Show>
        </section>
    }
}

/// A single embedded video with remove button.
#[component]
fn VideoEmbed<F>(video: VideoEntry, on_remove: F) -> impl IntoView
where
    F: Fn(String, String) + Clone + Send + 'static,
{
    let i18n = use_i18n();
    let video_id = video.id.clone();
    let video_rom_filename = video.rom_filename.clone();
    let on_remove = on_remove.clone();

    // Compute embed URL from platform and video_id
    let embed_url = match video.platform.as_str() {
        "youtube" => format!("https://www.youtube-nocookie.com/embed/{}", video.video_id),
        "twitch" => {
            // Twitch needs a parent param; use a placeholder that works
            format!(
                "https://player.twitch.tv/?video={}&parent=localhost",
                video.video_id
            )
        }
        "vimeo" => format!("https://player.vimeo.com/video/{}", video.video_id),
        "dailymotion" => format!("https://www.dailymotion.com/embed/video/{}", video.video_id),
        _ => video.url.clone(),
    };

    let title_display = video.title.clone().unwrap_or_default();

    view! {
        <div class="video-item">
            <div class="video-item-header">
                <span class="video-item-title">{title_display}</span>
                <button
                    class="video-remove-btn"
                    on:click=move |_| on_remove(video_id.clone(), video_rom_filename.clone())
                >
                    {move || t(i18n.locale.get(), Key::GameDetailRemoveVideo)}
                </button>
            </div>
            <div class="video-embed">
                <iframe
                    src=embed_url
                    sandbox="allow-scripts allow-same-origin allow-popups"
                    allowfullscreen=true
                ></iframe>
            </div>
        </div>
    }
}

/// Panel showing video search results with pin buttons.
#[component]
fn VideoRecommendations<F>(
    #[prop(into)] results: Signal<Vec<VideoRecommendation>>,
    is_searching: RwSignal<bool>,
    has_error: RwSignal<bool>,
    #[prop(into)] tag: Signal<String>,
    saved_videos: RwSignal<Vec<VideoEntry>>,
    on_pin: F,
) -> impl IntoView
where
    F: Fn(VideoRecommendation, String) + Clone + Send + 'static,
{
    let i18n = use_i18n();

    view! {
        <div class="video-recommendations">
            <Show when=move || has_error.get()>
                <p class="video-add-error">{move || t(i18n.locale.get(), Key::GameDetailSearchError)}</p>
            </Show>
            <Show when=move || !is_searching.get() && results.read().is_empty() && !has_error.get()>
                <p class="game-section-empty">{move || t(i18n.locale.get(), Key::GameDetailNoResults)}</p>
            </Show>
            <For
                each=move || results.get()
                key=|rec| rec.url.clone()
                let:rec
            >
                <RecommendationItem
                    rec=rec.clone()
                    tag=tag
                    saved_videos=saved_videos
                    on_pin=on_pin.clone()
                />
            </For>
        </div>
    }
}

fn visible_recommendations(
    results: &[VideoRecommendation],
    saved_videos: &[VideoEntry],
) -> Vec<VideoRecommendation> {
    results
        .iter()
        .filter(|rec| !is_video_saved(rec, saved_videos))
        .cloned()
        .collect()
}

fn is_video_saved(rec: &VideoRecommendation, saved_videos: &[VideoEntry]) -> bool {
    let parsed = replay_control_core::video_url::parse_video_url(&rec.url).ok();
    saved_videos.iter().any(|saved| {
        saved.url == rec.url
            || parsed.as_ref().is_some_and(|video| {
                saved.platform == video.platform.as_str() && saved.video_id == video.video_id
            })
            || rec.url.contains(&saved.video_id)
    })
}

/// A single recommendation result with thumbnail, inline player, and pin button.
#[component]
fn RecommendationItem<F>(
    rec: VideoRecommendation,
    tag: Signal<String>,
    saved_videos: RwSignal<Vec<VideoEntry>>,
    on_pin: F,
) -> impl IntoView
where
    F: Fn(VideoRecommendation, String) + Clone + Send + 'static,
{
    let i18n = use_i18n();
    let rec_sv = StoredValue::new(rec.clone());
    let playing = RwSignal::new(false);

    // Build embed URL from the YouTube watch URL
    let embed_url = StoredValue::new({
        rec.url
            .split("v=")
            .nth(1)
            .map(|id| {
                let id = id.split('&').next().unwrap_or(id);
                format!("https://www.youtube-nocookie.com/embed/{id}?autoplay=1")
            })
            .unwrap_or_default()
    });

    // Check if this video is already saved
    let is_pinned = move || {
        let url = &rec_sv.get_value().url;
        saved_videos
            .read()
            .iter()
            .any(|v| url.contains(&v.video_id))
    };

    let on_pin = on_pin.clone();
    let on_click_pin = move |_| {
        let r = rec_sv.get_value();
        let t = tag.get();
        on_pin(r, t);
    };

    let on_click_play = move |_| {
        playing.update(|p| *p = !*p);
    };

    let meta_text = StoredValue::new({
        let mut parts = Vec::new();
        if let Some(ref ch) = rec.channel {
            parts.push(ch.clone());
        }
        if let Some(ref dur) = rec.duration_text {
            parts.push(dur.clone());
        }
        parts.join(" \u{00B7} ")
    });

    view! {
        <div class="recommendation-item-wrapper">
            <div class="recommendation-item">
                <div class="recommendation-thumb-wrapper" on:click=on_click_play>
                    {rec.thumbnail_url.map(|url| view! {
                        <img class="recommendation-thumb" src=url alt="" />
                    })}
                    <div class="recommendation-play-icon">"\u{25B6}"</div>
                </div>
                <div class="recommendation-info" on:click=on_click_play>
                    <div class="recommendation-title">{rec.title.clone()}</div>
                    <Show when=move || !meta_text.get_value().is_empty()>
                        <div class="recommendation-meta">{meta_text.get_value()}</div>
                    </Show>
                </div>
                <button
                    class="recommendation-pin-btn"
                    class:pinned=is_pinned
                    prop:disabled=is_pinned
                    on:click=on_click_pin
                >
                    {move || {
                        if is_pinned() {
                            t(i18n.locale.get(), Key::GameDetailPinned)
                        } else {
                            t(i18n.locale.get(), Key::GameDetailPinVideo)
                        }
                    }}
                </button>
            </div>
            <Show when=move || playing.get()>
                <div class="recommendation-player">
                    <div class="video-embed">
                        <iframe
                            src=embed_url.get_value()
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
