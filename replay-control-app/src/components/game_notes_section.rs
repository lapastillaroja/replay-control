use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

fn format_relative_time(timestamp: u64) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let now = js_sys::Date::now() as u64 / 1000;
        let diff = now.saturating_sub(timestamp);
        if diff < 60 {
            format!("{}s ago", diff)
        } else if diff < 3600 {
            format!("{}m ago", diff / 60)
        } else if diff < 86400 {
            format!("{}h ago", diff / 3600)
        } else {
            format!("{}d ago", diff / 86400)
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        format!("{}d ago", timestamp / 86400)
    }
}

/// Notes section shown on the game detail page.
/// Allows the user to add free-form notes about their progress.
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
    let note_updated_at = RwSignal::new(Option::<u64>::None);
    let is_editing = RwSignal::new(false);
    let saving = RwSignal::new(false);
    let has_existing = RwSignal::new(false);

    let _sync = Effect::new(move || {
        if let Some(Ok(Some((text, updated_at)))) = note_resource.get() {
            note_text.set(text);
            note_updated_at.set(Some(updated_at));
            has_existing.set(true);
        } else if let Some(Ok(None)) = note_resource.get() {
            note_text.set(String::new());
            has_existing.set(false);
        }
    });

    let note_text_sv = StoredValue::new(note_text);
    let has_existing_sv = StoredValue::new(has_existing);

    let on_save = move |_| {
        let text = note_text_sv.get_value().get();
        if text.trim().is_empty() {
            return;
        }
        saving.set(true);
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        leptos::task::spawn_local(async move {
            if server_fns::set_game_note(sys, fname, text).await.is_ok() {
                saving.set(false);
                is_editing.set(false);
                has_existing.set(true);
            }
        });
    };

    let on_clear = move |_| {
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        note_text_sv.get_value().set(String::new());
        note_updated_at.set(None);
        has_existing.set(false);
        is_editing.set(false);
        leptos::task::spawn_local(async move {
            let _ = server_fns::clear_game_note(sys, fname).await;
        });
    };

    let on_edit = move |_| {
        is_editing.set(true);
    };

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
            ev.prevent_default();
            let text = note_text_sv.get_value().get();
            if text.trim().is_empty() {
                return;
            }
            saving.set(true);
            let sys = system.get_value();
            let fname = rom_filename.get_value();
            leptos::task::spawn_local(async move {
                if server_fns::set_game_note(sys, fname, text).await.is_ok() {
                    saving.set(false);
                    is_editing.set(false);
                    has_existing.set(true);
                }
            });
        } else if ev.key() == "Escape" {
            if !has_existing_sv.get_value().get() {
                on_clear(());
            } else {
                is_editing.set(false);
            }
        }
    };

    let on_cancel = move |_| {
        if !has_existing_sv.get_value().get() {
            on_clear(());
        } else {
            is_editing.set(false);
        }
    };

    let time_ago = move || note_updated_at.get().map(format_relative_time);

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameNotesTitle)}</h2>
            <Show when=move || has_existing.get() && !is_editing.get()
                fallback=move || view! {
                    <div class="game-notes-editor">
                        <textarea
                            class="game-notes-textarea"
                            prop:value=move || note_text_sv.get_value().get()
                            on:input=move |ev| note_text_sv.get_value().set(event_target_value(&ev))
                            on:keydown=on_keydown.clone()
                            placeholder=move || t(i18n.locale.get(), Key::GameNotesPlaceholder)
                            rows="3"
                        />
                        <div class="game-notes-actions">
                            <button
                                class="game-notes-btn game-notes-save"
                                prop:disabled=move || note_text_sv.get_value().get().trim().is_empty() || saving.get()
                                on:click=on_save
                            >
                                {move || if saving.get() {
                                    t(i18n.locale.get(), Key::GameNotesSaving)
                                } else {
                                    t(i18n.locale.get(), Key::GameNotesSave)
                                }}
                            </button>
                            <button
                                class="game-notes-btn game-notes-cancel"
                                on:click=on_cancel
                            >
                                {move || t(i18n.locale.get(), Key::CommonCancel)}
                            </button>
                        </div>
                    </div>
                }
            >
                <div class="game-notes-display">
                    <p class="game-note-text">{move || note_text_sv.get_value().get()}</p>
                    <div class="game-note-footer">
                        {move || time_ago().map(|ta| view! {
                            <span class="game-note-date">{ta}</span>
                        })}
                        <div class="game-note-actions">
                            <button
                                class="game-note-btn game-note-edit"
                                on:click=on_edit
                            >
                                {move || t(i18n.locale.get(), Key::GameNotesEdit)}
                            </button>
                            <button
                                class="game-note-btn game-note-clear"
                                on:click=move |_| { on_clear(()); }
                            >
                                {move || t(i18n.locale.get(), Key::GameNotesClear)}
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
            <Show when=move || !has_existing.get() && !is_editing.get()>
                <p class="game-section-empty">{move || t(i18n.locale.get(), Key::GameNotesEmpty)}</p>
                <button
                    class="game-notes-btn game-notes-add"
                    on:click=move |_| is_editing.set(true)
                >
                    {move || t(i18n.locale.get(), Key::GameNotesAdd)}
                </button>
            </Show>
        </section>
    }
}
