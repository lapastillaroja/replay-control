use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::boxart_placeholder::BoxArtPlaceholder;
use crate::hooks::{use_live_elapsed_secs, use_now_playing};
use crate::i18n::{Key, disc_label, play_state_label_key, t, use_i18n};
use crate::server_fns::{self, ReplayPlayerCommand};
use crate::types::NowPlayingState;
use crate::util::format_elapsed_short;
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
    let elapsed_text = Memo::new(move |_| format_elapsed_short(elapsed.get()).unwrap_or_default());

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
            <PlayerControls />
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
fn PlayerControls() -> impl IntoView {
    let i18n = use_i18n();
    let command = ServerAction::<server_fns::SendReplayPlayerCommand>::new();
    let pending = command.pending();

    let dispatch = move |player_command: ReplayPlayerCommand| {
        command.dispatch(server_fns::SendReplayPlayerCommand {
            command: player_command,
        });
    };
    let reset = move |_| {
        if confirm_reset(t(i18n.locale.get(), Key::PlayerControlResetConfirm)) {
            dispatch(ReplayPlayerCommand::GameReset);
        }
    };

    view! {
        <div class="now-playing-controls" aria-label=move || t(i18n.locale.get(), Key::CommonActions)>
            <PlayerControlButton
                label_key=Key::PlayerControlScreenshot
                pending
                on_press=Callback::new(move |()| dispatch(ReplayPlayerCommand::Screenshot))
            />
            <PlayerControlButton
                label_key=Key::PlayerControlHalt
                pending
                on_press=Callback::new(move |()| dispatch(ReplayPlayerCommand::Halt))
            />
            <PlayerControlButton
                label_key=Key::PlayerControlVolumeDown
                pending
                on_press=Callback::new(move |()| dispatch(ReplayPlayerCommand::VolumeDown))
            />
            <PlayerControlButton
                label_key=Key::PlayerControlMute
                pending
                on_press=Callback::new(move |()| dispatch(ReplayPlayerCommand::Mute))
            />
            <PlayerControlButton
                label_key=Key::PlayerControlVolumeUp
                pending
                on_press=Callback::new(move |()| dispatch(ReplayPlayerCommand::VolumeUp))
            />
            <button
                type="button"
                class="now-playing-control now-playing-control-reset"
                title=move || t(i18n.locale.get(), Key::PlayerControlReset)
                aria-label=move || t(i18n.locale.get(), Key::PlayerControlReset)
                prop:disabled=move || pending.get()
                on:click=reset
            >
                {move || t(i18n.locale.get(), Key::PlayerControlReset)}
            </button>
        </div>
    }
}

#[component]
fn PlayerControlButton(
    label_key: Key,
    #[prop(into)] pending: Signal<bool>,
    on_press: Callback<()>,
) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <button
            type="button"
            class="now-playing-control"
            title=move || t(i18n.locale.get(), label_key)
            aria-label=move || t(i18n.locale.get(), label_key)
            prop:disabled=move || pending.get()
            on:click=move |_| on_press.run(())
        >
            {move || t(i18n.locale.get(), label_key)}
        </button>
    }
}

fn confirm_reset(message: &str) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| window.confirm_with_message(message).ok())
            .unwrap_or(false)
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = message;
        true
    }
}
