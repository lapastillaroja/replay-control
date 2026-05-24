use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

fn retroachievements_credentials_complete(username: &str, password: &str) -> bool {
    username.trim().is_empty() == password.trim().is_empty()
}

#[component]
pub fn RetroAchievementsPage() -> impl IntoView {
    let i18n = use_i18n();
    let config = Resource::new_blocking(|| (), |_| server_fns::get_retroachievements_config());

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/settings" attr:class="back-btn">
                    {move || t(i18n.locale.get(), Key::GamesBack)}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), Key::RetroAchievementsTitle)}</h2>
            </div>

            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let config = config.await?;
                    Ok::<_, ServerFnError>(view! { <RetroAchievementsForm config /> })
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn RetroAchievementsForm(config: server_fns::RetroAchievementsConfig) -> impl IntoView {
    let i18n = use_i18n();

    let username = RwSignal::new(config.username);
    let password = RwSignal::new(String::new());
    let password_configured = RwSignal::new(config.password_configured);
    let show_password = RwSignal::new(false);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let save_credentials = move |clear: bool| {
        saving.set(true);
        status.set(None);

        let next_username = if clear { String::new() } else { username.get() };
        let next_password = if clear { String::new() } else { password.get() };
        let locale = i18n.locale.get_untracked();

        leptos::task::spawn_local(async move {
            match server_fns::save_retroachievements_config_and_restart(
                next_username.clone(),
                next_password.clone(),
            )
            .await
            {
                Ok(msg) => {
                    username.set(next_username);
                    password.set(String::new());
                    password_configured.set(!next_password.trim().is_empty());
                    status.set(Some((
                        true,
                        format!("{}: {msg}", t(locale, Key::RetroAchievementsSaved)),
                    )));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    let on_save = move |_| {
        let locale = i18n.locale.get_untracked();
        if !retroachievements_credentials_complete(
            &username.get_untracked(),
            &password.get_untracked(),
        ) {
            status.set(Some((
                false,
                t(locale, Key::RetroAchievementsCredentialsRequired).to_string(),
            )));
            return;
        }

        save_credentials(false);
    };

    let on_clear = move |_| {
        save_credentials(true);
    };

    view! {
        <div class="settings-form">
            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), Key::RetroAchievementsUsername)}</label>
                <input
                    type="text"
                    class="form-input"
                    bind:value=username
                    autocomplete="username"
                />
            </div>

            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), Key::RetroAchievementsPassword)}</label>
                <div class="input-with-toggle">
                    <input
                        type=move || if show_password.get() { "text" } else { "password" }
                        class="form-input"
                        bind:value=password
                        autocomplete="current-password"
                        placeholder=move || t(i18n.locale.get(), Key::SettingsPasswordEnter)
                    />
                    <button
                        type="button"
                        class="toggle-password"
                        on:click=move |_| show_password.update(|v| *v = !*v)
                    >
                        {move || if show_password.get() { "\u{1F648}" } else { "\u{1F441}" }}
                    </button>
                </div>
                <p class="form-hint">
                    {move || {
                        let key = if password_configured.get() {
                            Key::RetroAchievementsPasswordSaved
                        } else {
                            Key::RetroAchievementsPasswordMissing
                        };
                        t(i18n.locale.get(), key)
                    }}
                </p>
            </div>

            <p class="form-hint">{move || t(i18n.locale.get(), Key::SettingsReplayRestartWarning)}</p>

            {move || status.get().map(|(ok, msg)| {
                let class = if ok { "status-msg status-ok" } else { "status-msg status-err" };
                view! { <div class=class>{msg}</div> }
            })}

            <button
                class="form-btn"
                on:click=on_save
                disabled=move || saving.get()
            >
                {move || {
                    let locale = i18n.locale.get();
                    if saving.get() {
                        t(locale, Key::SettingsRestarting)
                    } else {
                        t(locale, Key::RetroAchievementsSaveRestart)
                    }
                }}
            </button>

            <div class="apply-section">
                <button
                    class="form-btn form-btn-secondary"
                    on:click=on_clear
                    disabled=move || saving.get()
                >
                    {move || {
                        let locale = i18n.locale.get();
                        if saving.get() {
                            t(locale, Key::SettingsRestarting)
                        } else {
                            t(locale, Key::RetroAchievementsClearRestart)
                        }
                    }}
                </button>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::retroachievements_credentials_complete;

    #[test]
    fn retroachievements_credentials_are_all_or_nothing() {
        assert!(retroachievements_credentials_complete("", ""));
        assert!(retroachievements_credentials_complete("player", "secret"));
        assert!(retroachievements_credentials_complete(
            "  player  ",
            "  secret  "
        ));
        assert!(!retroachievements_credentials_complete("player", ""));
        assert!(!retroachievements_credentials_complete("", "secret"));
        assert!(!retroachievements_credentials_complete("player", "   "));
    }
}
