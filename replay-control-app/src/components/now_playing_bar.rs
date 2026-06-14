use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::boxart_placeholder::BoxArtPlaceholder;
use crate::hooks::{Clock, use_live_elapsed_secs, use_now_playing};
use crate::i18n::{Key, disc_label, play_state_label_key, t, tf, use_i18n};
use crate::server_fns::{self, ReplayPlayerCommand};
use crate::types::NowPlayingState;
use crate::util::{confirm_action, format_elapsed_short};
use replay_control_core::locale::Locale;
use replay_control_core::replay_api::{DiscInfo, PlayState};

#[component]
pub fn NowPlayingBar() -> impl IntoView {
    let now_playing = use_now_playing();

    view! {
        <Show
            when=move || matches!(now_playing.get(), NowPlayingState::Playing { .. })
            fallback=|| ()
        >
            {move || match now_playing.get() {
                NowPlayingState::Playing {
                    system,
                    system_display,
                    filename,
                    display_name,
                    box_art_url,
                    started_at_unix_secs,
                    play_state,
                    disc,
                } => view! {
                    <ActiveNowPlayingBar
                        system
                        system_display
                        filename
                        display_name
                        box_art_url
                        started_at_unix_secs
                        play_state
                        disc
                    />
                }
                .into_any(),
                _ => ().into_any(),
            }}
        </Show>
    }
}

#[component]
fn ActiveNowPlayingBar(
    system: String,
    system_display: String,
    filename: String,
    display_name: String,
    box_art_url: Option<String>,
    started_at_unix_secs: u64,
    play_state: PlayState,
    disc: Option<DiscInfo>,
) -> impl IntoView {
    let i18n = use_i18n();
    let elapsed = use_live_elapsed_secs(started_at_unix_secs);
    let href = format!(
        "/games/{}/{}",
        urlencoding::encode(&system),
        urlencoding::encode(&filename),
    );
    let title = display_name.clone();
    let placeholder_system = system.clone();
    let placeholder_name = display_name.clone();
    let state_class = play_state_class(play_state);
    let elapsed_text = Memo::new(move |_| format_elapsed_short(elapsed.get()));
    let save_state_system = system.clone();
    let save_state_filename = filename.clone();
    let command = ServerAction::<server_fns::SendReplayPlayerCommand>::new();
    let pending = command.pending();
    let more_open = RwSignal::new(false);
    let dispatch = Callback::new(move |player_command: ReplayPlayerCommand| {
        command.dispatch(server_fns::SendReplayPlayerCommand {
            command: player_command,
        });
    });

    view! {
        <section class="now-playing-bar" aria-label=move || t(i18n.locale.get(), Key::HomeNowPlaying)>
            <A href=href.clone() attr:class="now-playing-bar-main" attr:title=title>
                <div class="now-playing-bar-art" aria-hidden="true">
                    {match box_art_url {
                        Some(url) => view! {
                            <img class="now-playing-bar-thumb" src=url loading="lazy" alt="" />
                        }.into_any(),
                        None => view! {
                            <BoxArtPlaceholder
                                system=placeholder_system
                                name=placeholder_name
                                size="list".to_string()
                            />
                        }.into_any(),
                    }}
                </div>
                <div class="now-playing-bar-body">
                    <div class="now-playing-bar-kicker">
                        <span class=format!("now-playing-bar-dot {state_class}") aria-hidden="true"></span>
                        <span>{move || t(i18n.locale.get(), Key::HomeNowPlaying)}</span>
                    </div>
                    <div class="now-playing-bar-title">{display_name}</div>
                    <div class="now-playing-bar-meta">
                        <span>{system_display}</span>
                        <span>{move || t(i18n.locale.get(), play_state_label_key(play_state))}</span>
                        <span class="now-playing-bar-elapsed">{move || elapsed_text.get()}</span>
                        {move || disc.map(|disc| {
                            view! {
                                <span class="now-playing-bar-disc">
                                    {disc_label(i18n.locale.get(), disc)}
                                </span>
                            }
                        })}
                    </div>
                </div>
            </A>
            <PlayerControls pending dispatch more_open />
            <Show when=move || more_open.get() fallback=|| ()>
                <NowPlayingMorePanel
                    pending
                    dispatch
                    system=save_state_system.clone()
                    filename=save_state_filename.clone()
                />
            </Show>
        </section>
    }
}

fn play_state_class(play_state: PlayState) -> &'static str {
    match play_state {
        PlayState::Playing => "is-playing",
        PlayState::Paused => "is-paused",
        PlayState::Halted => "is-halted",
        PlayState::InMenu => "is-in-menu",
    }
}

#[component]
fn PlayerControls(
    #[prop(into)] pending: Signal<bool>,
    dispatch: Callback<ReplayPlayerCommand>,
    more_open: RwSignal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="now-playing-controls" aria-label=move || t(i18n.locale.get(), Key::CommonActions)>
            <div class="now-playing-controls-row">
                <PlayerControlButton
                    label_key=Key::PlayerControlScreenshot
                    pending
                    icon=PlayerControlIcon::Screenshot
                    on_press=Callback::new(move |()| dispatch.run(ReplayPlayerCommand::Screenshot))
                />
                <PlayerControlButton
                    label_key=Key::PlayerControlVolumeDown
                    pending
                    icon=PlayerControlIcon::VolumeDown
                    on_press=Callback::new(move |()| dispatch.run(ReplayPlayerCommand::VolumeDown))
                />
                <PlayerControlButton
                    label_key=Key::PlayerControlMute
                    pending
                    icon=PlayerControlIcon::Mute
                    on_press=Callback::new(move |()| dispatch.run(ReplayPlayerCommand::Mute))
                />
                <PlayerControlButton
                    label_key=Key::PlayerControlVolumeUp
                    pending
                    icon=PlayerControlIcon::VolumeUp
                    on_press=Callback::new(move |()| dispatch.run(ReplayPlayerCommand::VolumeUp))
                />
                <PlayerControlButton
                    label_key=Key::PlayerControlHalt
                    pending
                    icon=PlayerControlIcon::Halt
                    on_press=Callback::new(move |()| dispatch.run(ReplayPlayerCommand::Halt))
                />
                <PlayerControlButton
                    label_key=Key::PlayerControlReset
                    pending
                    extra_class="now-playing-control-reset"
                    icon=PlayerControlIcon::Reset
                    on_press=Callback::new(move |()| {
                        if confirm_action(t(i18n.locale.get(), Key::PlayerControlResetConfirm)) {
                            dispatch.run(ReplayPlayerCommand::GameReset);
                        }
                    })
                />
                <button
                    type="button"
                    class="now-playing-control now-playing-control-more now-playing-control-icon-only"
                    class:is-open=move || more_open.get()
                    title=move || t(i18n.locale.get(), Key::PlayerControlMore)
                    aria-label=move || t(i18n.locale.get(), Key::PlayerControlMore)
                    aria-expanded=move || more_open.get().to_string()
                    on:click=move |_| {
                        more_open.update(|open| *open = !*open);
                        release_active_element();
                    }
                >
                    <span class="now-playing-more-dots" aria-hidden="true">{"..."}</span>
                </button>
            </div>
        </div>
    }
}

#[component]
fn NowPlayingMorePanel(
    #[prop(into)] pending: Signal<bool>,
    dispatch: Callback<ReplayPlayerCommand>,
    system: String,
    filename: String,
) -> impl IntoView {
    view! {
        <div class="now-playing-more-panel">
            <SaveStatesPanel pending dispatch system filename />
        </div>
    }
}

#[component]
fn SaveStatesPanel(
    #[prop(into)] pending: Signal<bool>,
    dispatch: Callback<ReplayPlayerCommand>,
    system: String,
    filename: String,
) -> impl IntoView {
    let i18n = use_i18n();
    let slot = RwSignal::new(1_u8);
    let refresh = RwSignal::new(0_u64);
    let save_slots = Resource::new(
        move || (system.clone(), filename.clone(), refresh.get()),
        |(system, filename, _)| server_fns::get_save_state_slots(system, filename),
    );
    let decrement = move |_| {
        slot.update(|value| *value = value.saturating_sub(1).clamp(1, 18));
        release_active_element();
    };
    let increment = move |_| {
        slot.update(|value| *value = value.saturating_add(1).clamp(1, 18));
        release_active_element();
    };
    let loading_state = move || t(i18n.locale.get(), Key::SaveStatesStatusPreview).to_string();

    view! {
        <div class="now-playing-save-states">
            <SaveStatesSlotStepper selected_slot=slot decrement increment />
            <Suspense fallback=move || view! {
                <SaveStatesSlotSummary selected_slot=slot state=Signal::derive(loading_state) />
                <SaveStatesActionsLoading />
            }>
                {move || Suspend::new(async move {
                    let snapshot = match save_slots.await {
                        Ok(slots) => SaveSlotsSnapshot::Ready(slots),
                        Err(_) => SaveSlotsSnapshot::Error,
                    };
                    view! {
                        <SaveStatesLoadedControls
                            pending
                            dispatch
                            selected_slot=slot
                            refresh
                            snapshot
                        />
                    }
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn SaveStatesLoadedControls(
    #[prop(into)] pending: Signal<bool>,
    dispatch: Callback<ReplayPlayerCommand>,
    selected_slot: RwSignal<u8>,
    refresh: RwSignal<u64>,
    snapshot: SaveSlotsSnapshot,
) -> impl IntoView {
    let i18n = use_i18n();
    let clock = use_context::<Clock>();
    let slots = StoredValue::new(snapshot);
    let selected_lookup =
        Memo::new(move |_| selected_save_slot(slots.get_value(), selected_slot.get()));
    let slot_state = move || {
        let locale = i18n.locale.get();
        match selected_lookup.get() {
            SaveSlotLookup::Unknown => t(locale, Key::SaveStatesStatusPreview).to_string(),
            SaveSlotLookup::Empty => t(locale, Key::SaveStatesEmpty).to_string(),
            SaveSlotLookup::Occupied(modified_unix_secs) => {
                let now = clock
                    .map(|clock| clock.now())
                    .filter(|now| *now > 0)
                    .or_else(now_unix_secs);
                format_save_state_timestamp(locale, modified_unix_secs, now)
            }
        }
    };
    let slot_is_empty = move || {
        matches!(
            selected_lookup.get(),
            SaveSlotLookup::Unknown | SaveSlotLookup::Empty
        )
    };
    let on_save = move |_| {
        let slot_number = selected_slot.get_untracked();
        let selected = selected_lookup.get_untracked();
        if let SaveSlotLookup::Occupied(modified_unix_secs) = selected {
            let locale = i18n.locale.get_untracked();
            let now = clock
                .map(|clock| clock.now())
                .filter(|now| *now > 0)
                .or_else(now_unix_secs);
            let state = format_save_state_timestamp(locale, modified_unix_secs, now);
            let target = save_state_target_label(locale, slot_number, &state);
            let message = save_state_confirm_message(
                locale,
                Key::SaveStatesOverwriteTitle,
                Key::SaveStatesOverwriteBody,
                &target,
            );
            if confirm_action(&message) {
                dispatch.run(ReplayPlayerCommand::SaveState { slot: slot_number });
                schedule_save_state_refresh(refresh);
            }
        } else {
            dispatch.run(ReplayPlayerCommand::SaveState { slot: slot_number });
            schedule_save_state_refresh(refresh);
        }
        release_active_element();
    };
    let on_load = move |_| {
        let slot_number = selected_slot.get_untracked();
        if let SaveSlotLookup::Occupied(_) = selected_lookup.get_untracked() {
            let locale = i18n.locale.get_untracked();
            let state = slot_state();
            let target = save_state_target_label(locale, slot_number, &state);
            let message = save_state_confirm_message(
                locale,
                Key::SaveStatesLoadTitle,
                Key::SaveStatesLoadBody,
                &target,
            );
            if confirm_action(&message) {
                dispatch.run(ReplayPlayerCommand::LoadState { slot: slot_number });
            }
        }
        release_active_element();
    };

    view! {
        <SaveStatesSlotSummary selected_slot state=Signal::derive(slot_state) />
        <div class="save-states-actions">
            <button
                type="button"
                class="now-playing-control save-states-action"
                aria-label=move || t(i18n.locale.get(), Key::SaveStatesSave)
                prop:disabled=move || pending.get()
                on:click=on_save
            >
                <PlayerControlSvgIcon icon=PlayerControlIcon::Save />
                <span class="visually-hidden">{move || t(i18n.locale.get(), Key::SaveStatesSave)}</span>
            </button>
            <button
                type="button"
                class="now-playing-control save-states-action"
                aria-label=move || t(i18n.locale.get(), Key::SaveStatesLoad)
                prop:disabled=move || pending.get() || slot_is_empty()
                on:click=on_load
            >
                <PlayerControlSvgIcon icon=PlayerControlIcon::Load />
                <span class="visually-hidden">{move || t(i18n.locale.get(), Key::SaveStatesLoad)}</span>
            </button>
        </div>
    }
}

#[component]
fn SaveStatesSlotStepper(
    selected_slot: RwSignal<u8>,
    decrement: impl Fn(leptos::ev::MouseEvent) + 'static,
    increment: impl Fn(leptos::ev::MouseEvent) + 'static,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="save-states-step-group">
            <button
                type="button"
                class="now-playing-control now-playing-control-icon-only save-states-step"
                aria-label=move || t(i18n.locale.get(), Key::SaveStatesPreviousSlot)
                on:click=decrement
                disabled=move || selected_slot.get() == 1
            >
                <PlayerControlSvgIcon icon=PlayerControlIcon::Minus />
            </button>
            <button
                type="button"
                class="now-playing-control now-playing-control-icon-only save-states-step"
                aria-label=move || t(i18n.locale.get(), Key::SaveStatesNextSlot)
                on:click=increment
                disabled=move || selected_slot.get() == 18
            >
                <PlayerControlSvgIcon icon=PlayerControlIcon::Plus />
            </button>
        </div>
    }
}

#[component]
fn SaveStatesSlotSummary(
    selected_slot: RwSignal<u8>,
    #[prop(into)] state: Signal<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="save-states-slot">
            <span class="save-states-slot-label">
                {move || format!("{} {}/18", t(i18n.locale.get(), Key::SaveStatesSlot), selected_slot.get())}
            </span>
            <span class="save-states-slot-separator">{"-"}</span>
            <span class="save-states-slot-state">
                {move || state.get()}
            </span>
        </div>
    }
}

#[component]
fn SaveStatesActionsLoading() -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="save-states-actions">
            <button
                type="button"
                class="now-playing-control save-states-action"
                aria-label=move || t(i18n.locale.get(), Key::SaveStatesSave)
                prop:disabled=true
            >
                <PlayerControlSvgIcon icon=PlayerControlIcon::Save />
                <span class="visually-hidden">{move || t(i18n.locale.get(), Key::SaveStatesSave)}</span>
            </button>
            <button
                type="button"
                class="now-playing-control save-states-action"
                aria-label=move || t(i18n.locale.get(), Key::SaveStatesLoad)
                prop:disabled=true
            >
                <PlayerControlSvgIcon icon=PlayerControlIcon::Load />
                <span class="visually-hidden">{move || t(i18n.locale.get(), Key::SaveStatesLoad)}</span>
            </button>
        </div>
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveSlotLookup {
    Unknown,
    Empty,
    Occupied(u64),
}

#[derive(Debug, Clone)]
enum SaveSlotsSnapshot {
    Ready(Vec<server_fns::SaveStateSlotStatus>),
    Error,
}

fn selected_save_slot(slots: SaveSlotsSnapshot, slot: u8) -> SaveSlotLookup {
    let SaveSlotsSnapshot::Ready(slots) = slots else {
        return SaveSlotLookup::Unknown;
    };
    slots
        .iter()
        .find(|status| status.slot == slot)
        .and_then(|status| status.modified_unix_secs)
        .map(SaveSlotLookup::Occupied)
        .unwrap_or(SaveSlotLookup::Empty)
}

fn format_save_state_timestamp(
    locale: Locale,
    modified_unix_secs: u64,
    now_unix_secs: Option<u64>,
) -> String {
    let Some(now) = now_unix_secs else {
        return t(locale, Key::SaveStatesSaved).to_string();
    };
    if modified_unix_secs <= now {
        let elapsed = now - modified_unix_secs;
        if elapsed < 60 {
            return t(locale, Key::SaveStatesJustNow).to_string();
        }
        if elapsed < 3600 {
            let minutes = (elapsed / 60).max(1).to_string();
            return tf(locale, Key::SaveStatesMinutesAgo, &[&minutes]);
        }
    }
    format_save_state_absolute_time(locale, modified_unix_secs)
}

fn format_save_state_absolute_time(locale: Locale, modified_unix_secs: u64) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = locale;
        let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(
            modified_unix_secs as f64 * 1000.0,
        ));
        let year = date.get_full_year();
        let month = date.get_month() + 1;
        let day = date.get_date();
        let hours = date.get_hours();
        let minutes = date.get_minutes();
        return format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}");
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = modified_unix_secs;
        t(locale, Key::SaveStatesSaved).to_string()
    }
}

fn now_unix_secs() -> Option<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        return Some((js_sys::Date::now() / 1000.0) as u64);
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs())
    }
}

fn schedule_save_state_refresh(refresh: RwSignal<u64>) {
    #[cfg(feature = "hydrate")]
    {
        gloo_timers::callback::Timeout::new(1_200, move || {
            refresh.update(|version| *version = version.saturating_add(1));
        })
        .forget();
    }

    #[cfg(not(feature = "hydrate"))]
    {
        refresh.update(|version| *version = version.saturating_add(1));
    }
}

fn save_state_target_label(locale: Locale, slot: u8, state: &str) -> String {
    format!("{} {}/18 - {}", t(locale, Key::SaveStatesSlot), slot, state,)
}

fn save_state_confirm_message(
    locale: Locale,
    title_key: Key,
    body_key: Key,
    target: &str,
) -> String {
    format!(
        "{}\n\n{}",
        t(locale, title_key),
        tf(locale, body_key, &[target])
    )
}

#[component]
fn PlayerControlButton(
    label_key: Key,
    #[prop(into)] pending: Signal<bool>,
    #[prop(optional)] icon: Option<PlayerControlIcon>,
    #[prop(optional)] extra_class: &'static str,
    on_press: Callback<()>,
) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <button
            type="button"
            class=move || format!("now-playing-control {extra_class}")
            class:now-playing-control-icon-only=move || icon.is_some()
            title=move || t(i18n.locale.get(), label_key)
            aria-label=move || t(i18n.locale.get(), label_key)
            prop:disabled=move || pending.get()
            on:click=move |_| {
                on_press.run(());
                release_active_element();
            }
        >
            {match icon {
                Some(icon) => view! {
                    <PlayerControlSvgIcon icon />
                    <span class="visually-hidden">{move || t(i18n.locale.get(), label_key)}</span>
                }.into_any(),
                None => view! {
                    <span>{move || t(i18n.locale.get(), label_key)}</span>
                }.into_any(),
            }}
        </button>
    }
}

#[derive(Clone, Copy)]
enum PlayerControlIcon {
    Screenshot,
    Halt,
    Reset,
    Save,
    Load,
    Minus,
    Plus,
    VolumeDown,
    VolumeUp,
    Mute,
}

/// Audio controls use Lucide-compatible stroke icons (ISC licensed), rendered
/// inline so they inherit the current skin/accent colors without another
/// frontend dependency.
#[component]
fn PlayerControlSvgIcon(icon: PlayerControlIcon) -> impl IntoView {
    match icon {
        PlayerControlIcon::Screenshot => view! {
            <svg class="now-playing-control-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z"></path>
                <circle cx="12" cy="13" r="3"></circle>
            </svg>
        }.into_any(),
        PlayerControlIcon::Halt => view! {
            <svg class="now-playing-control-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <rect x="6" y="4" width="4" height="16" rx="1"></rect>
                <rect x="14" y="4" width="4" height="16" rx="1"></rect>
            </svg>
        }.into_any(),
        PlayerControlIcon::Reset => view! {
            <svg class="now-playing-control-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M3 12a9 9 0 1 0 3-6.7"></path>
                <path d="M3 3v6h6"></path>
            </svg>
        }.into_any(),
        PlayerControlIcon::Save => view! {
            <svg class="now-playing-control-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"></path>
                <path d="M7 3v5h8"></path>
                <path d="M12 12v7"></path>
                <path d="m8.5 15.5 3.5-3.5 3.5 3.5"></path>
            </svg>
        }.into_any(),
        PlayerControlIcon::Load => view! {
            <svg class="now-playing-control-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"></path>
                <path d="M7 3v5h8"></path>
                <path d="M12 19v-7"></path>
                <path d="m8.5 15.5 3.5 3.5 3.5-3.5"></path>
            </svg>
        }.into_any(),
        PlayerControlIcon::Minus => view! {
            <svg class="now-playing-control-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M5 12h14"></path>
            </svg>
        }.into_any(),
        PlayerControlIcon::Plus => view! {
            <svg class="now-playing-control-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M12 5v14"></path>
                <path d="M5 12h14"></path>
            </svg>
        }.into_any(),
        PlayerControlIcon::VolumeDown => view! {
            <svg class="now-playing-control-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"></polygon>
                <path d="M15.5 8.5a5 5 0 0 1 0 7"></path>
            </svg>
        }.into_any(),
        PlayerControlIcon::VolumeUp => view! {
            <svg class="now-playing-control-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"></polygon>
                <path d="M15.5 8.5a5 5 0 0 1 0 7"></path>
                <path d="M19 5a9 9 0 0 1 0 14"></path>
            </svg>
        }.into_any(),
        PlayerControlIcon::Mute => view! {
            <svg class="now-playing-control-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"></polygon>
                <path d="M22 9 16 15"></path>
                <path d="m16 9 6 6"></path>
            </svg>
        }.into_any(),
    }
}

fn release_active_element() {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;

        if let Some(element) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.active_element())
            .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = element.blur();
        }
    }
}
