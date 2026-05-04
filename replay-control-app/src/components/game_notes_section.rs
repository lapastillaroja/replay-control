use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

/// Notes section shown on the game detail page.
/// Always shows a textarea — save with the button or Ctrl/Cmd+Enter.
#[component]
pub fn GameNotesSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    let note_resource = Resource::new(
        move || (system.get_value(), rom_filename.get_value()),
        move |(sys, fname)| server_fns::get_game_note(sys, fname),
    );

    let note_text = RwSignal::new(String::new());
    let saving = RwSignal::new(false);

    let _sync = Effect::new(move || {
        if let Some(Ok(Some((text, _)))) = note_resource.get() {
            note_text.set(text);
        }
    });

    let do_save = move || {
        let text = note_text.get_untracked();
        saving.set(true);
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        leptos::task::spawn_local(async move {
            if text.trim().is_empty() {
                let _ = server_fns::clear_game_note(sys, fname).await;
            } else {
                let _ = server_fns::set_game_note(sys, fname, text).await;
            }
            saving.set(false);
        });
    };

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            ev.prevent_default();
            do_save();
        }
    };

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameNotesTitle)}</h2>
            <div class="game-notes-editor">
                <textarea
                    class="game-notes-textarea"
                    prop:value=move || note_text.get()
                    on:input=move |ev| note_text.set(event_target_value(&ev))
                    on:keydown=on_keydown
                    placeholder=move || t(i18n.locale.get(), Key::GameNotesPlaceholder)
                    rows="3"
                />
                <div class="game-notes-actions">
                    <button
                        class="game-notes-btn game-notes-save"
                        disabled=move || saving.get()
                        on:click=move |_| do_save()
                    >
                        {move || if saving.get() {
                            t(i18n.locale.get(), Key::GameNotesSaving)
                        } else {
                            t(i18n.locale.get(), Key::GameNotesSave)
                        }}
                    </button>
                </div>
            </div>
        </section>
    }
}
