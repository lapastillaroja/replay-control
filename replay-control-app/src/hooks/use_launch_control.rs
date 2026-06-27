use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::hooks::use_now_playing;
use crate::i18n::{Key, Locale, tf, use_i18n};
use crate::server_fns;
use crate::types::NowPlayingState;
use crate::util::confirm_action;

/// Shared launch-button state + click handler for a single game row.
///
/// The launch `<button>` markup stays inline in each row (a shared child
/// component lost taps on iOS Safari after a swipe-back), but the handler logic
/// and its two state signals carry no such constraint, so they live here and
/// are reused by both the game list and the favorites list.
pub struct LaunchControl {
    pub launching: RwSignal<bool>,
    pub launch_failed: RwSignal<bool>,
    pub on_launch: Callback<()>,
}

pub fn use_launch_control(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    rom_path: StoredValue<String>,
    row_label: StoredValue<String>,
) -> LaunchControl {
    let now_playing = use_now_playing();
    let i18n = use_i18n();
    let launching = RwSignal::new(false);
    let launch_failed = RwSignal::new(false);

    let on_launch = Callback::new(move |_: ()| {
        if launching.get_untracked() {
            return;
        }
        // If a different game is running, confirm before replacing it. Read the
        // raw signal untracked so this survives an iOS Safari bfcache restore:
        // an owner-bound (derived) read silently aborts the handler after back.
        if let NowPlayingState::Playing {
            system: cur_system,
            filename: cur_filename,
            display_name: cur_name,
            ..
        } = now_playing.get_untracked()
            && (cur_system != system.get_value() || cur_filename != rom_filename.get_value())
            && !confirm_replace_running_game(i18n.locale.get(), &cur_name, &row_label.get_value())
        {
            return;
        }
        launching.set(true);
        launch_failed.set(false);
        let path = rom_path.get_value();
        spawn_local(async move {
            let failed = server_fns::launch_game(path, String::new()).await.is_err();
            launching.set(false);
            launch_failed.set(failed);
        });
    });

    LaunchControl {
        launching,
        launch_failed,
        on_launch,
    }
}

/// If a different game is already running, confirm before replacing it.
/// Returns true to proceed with the launch, false to cancel.
fn confirm_replace_running_game(locale: Locale, current_name: &str, next_name: &str) -> bool {
    let message = tf(
        locale,
        Key::LaunchReplaceConfirm,
        &[next_name, current_name],
    );
    confirm_action(&message)
}
