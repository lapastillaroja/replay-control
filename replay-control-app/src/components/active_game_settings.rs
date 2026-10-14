use crate::components::client_only::ClientOnly;
use crate::components::confirm_dialog::use_confirm_dialog;
use crate::components::status_message::StatusMessage;
use crate::components::step_button::StepButton;
use crate::i18n::{Key, Locale, t, use_i18n};
use crate::server_fns;
#[cfg(feature = "hydrate")]
use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use replay_control_core::replay_settings::{
    ActiveGameIdentity, ActiveOverrideConfig, ActiveOverrideUpdate, OverrideChange, OverrideScope,
    OverrideSettingsOutcome, ReplaySetting,
};

type GameDefaults = StoredValue<Vec<(ReplaySetting, RwSignal<Option<i32>>)>>;

#[component]
pub fn ActiveGameSettings(system: String, rom_path: String) -> impl IntoView {
    let i18n = use_i18n();
    let system = StoredValue::new(system);
    let rom_path = StoredValue::new(rom_path);
    let refresh = RwSignal::new(0u64);
    let scope = RwSignal::new(OverrideScope::Game);
    let game_notice = RwSignal::new(None::<(bool, String)>);
    let system_notice = RwSignal::new(None::<(bool, String)>);
    let config = LocalResource::new(move || {
        refresh.get();
        server_fns::get_active_override_configs()
    });
    let reload = Callback::new(move |()| refresh.update(|value| *value += 1));
    view! {
        <section class="section">
          <div class="active-game-settings-panel">
            <div class="active-game-settings-header">
              <h2 class="section-title">{move || t(i18n.locale.get(), Key::GameSettingsTitle)}</h2>
              <div class="active-game-settings-scopes" role="group">
                <button type="button" class="compact-control"
                    class:is-active=move || scope.get() == OverrideScope::Game
                    aria-pressed=move || if scope.get() == OverrideScope::Game { "true" } else { "false" }
                    on:click=move |_| scope.set(OverrideScope::Game)>
                    {move || t(i18n.locale.get(), Key::GameSettingsGameScope)}
                </button>
                <button type="button" class="compact-control"
                    class:is-active=move || scope.get() == OverrideScope::System
                    aria-pressed=move || if scope.get() == OverrideScope::System { "true" } else { "false" }
                    on:click=move |_| scope.set(OverrideScope::System)>
                    {move || t(i18n.locale.get(), Key::GameSettingsSystemScope)}
                </button>
              </div>
            </div>
            <p class="form-hint">{move || t(i18n.locale.get(), if scope.get() == OverrideScope::Game { Key::GameSettingsHint } else { Key::SystemSettingsHint })}</p>
            <div hidden=move || scope.get() != OverrideScope::Game><StatusMessage status=game_notice dismiss_success_after_ms=5_000 /></div>
            <div hidden=move || scope.get() != OverrideScope::System><StatusMessage status=system_notice dismiss_success_after_ms=5_000 /></div>
            <ClientOnly fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
            <ErrorBoundary fallback=move |_| view! {
                <p class="status-msg status-err">{move || t(i18n.locale.get(), Key::GameSettingsUnavailable)}</p>
                <button type="button" class="compact-control" on:click=move |_| reload.run(())>
                    {move || t(i18n.locale.get(), Key::ReplayApiCheckAgain)}
                </button>
            }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                    {move || Suspend::new(async move {
                        let data = config.await?;
                        // The page may lag behind TV state. Never show a new game's
                        // settings underneath the previous game's title.
                        if !data.game.identity.matches_library_game(&system.get_value(), &rom_path.get_value()) {
                            return Ok::<_, server_fn::ServerFnError>(view! {
                                <p class="form-hint">{move || t(i18n.locale.get(), Key::GameSettingsChanged)}</p>
                            }.into_any());
                        }
                        let game_settings_saved = RwSignal::new(data.game.configured);
                        let saving = RwSignal::new(false);
                        let game_defaults = StoredValue::new(data.game.overrides.iter().map(|entry| {
                            (entry.setting, RwSignal::new(entry.inherited))
                        }).collect());
                        Ok(view! {
                            <div class="active-game-settings-scope-content" hidden=move || scope.get() != OverrideScope::Game>
                                <ActiveGameSettingsForm config=data.game reload notice=game_notice game_settings_saved game_defaults saving />
                            </div>
                            <div class="active-game-settings-scope-content" hidden=move || scope.get() != OverrideScope::System>
                                <ActiveGameSettingsForm config=data.system reload notice=system_notice game_settings_saved game_defaults saving />
                            </div>
                        }.into_any())
                    })}
                </Suspense>
            </ErrorBoundary>
            </ClientOnly>
          </div>
        </section>
    }
}

#[component]
fn ActiveGameSettingsForm(
    config: ActiveOverrideConfig,
    reload: Callback<()>,
    notice: RwSignal<Option<(bool, String)>>,
    game_settings_saved: RwSignal<bool>,
    game_defaults: GameDefaults,
    saving: RwSignal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let scope = config.scope;
    let settings_saved = if scope == OverrideScope::Game {
        game_settings_saved
    } else {
        RwSignal::new(config.configured)
    };
    let identity = StoredValue::new(config.identity);
    let entries: Vec<_> = config
        .overrides
        .into_iter()
        .map(|entry| {
            let inherited = if scope == OverrideScope::Game {
                game_defaults.with_value(|defaults| {
                    defaults
                        .iter()
                        .find(|(setting, _)| *setting == entry.setting)
                        .map(|(_, value)| *value)
                        .expect("game setting has inherited value")
                })
            } else {
                RwSignal::new(entry.inherited)
            };
            (
                entry.setting,
                inherited,
                entry.global_default,
                RwSignal::new(entry.value),
                RwSignal::new(entry.value),
            )
        })
        .collect();
    let values = StoredValue::new(entries.clone());
    let has_overrides = move || {
        values.with_value(|entries| {
            entries
                .iter()
                .any(|(_, _, _, _, value)| value.get().is_some())
        })
    };
    let has_changes = move || {
        values.with_value(|entries| {
            entries
                .iter()
                .any(|(_, _, _, original, value)| original.get() != value.get())
        })
    };
    let reset_all = move |_| {
        notice.set(None);
        values.with_value(|entries| {
            for (_, _, _, _, value) in entries {
                value.set(None);
            }
        });
    };
    let save = move |_| {
        if saving.get_untracked() || !has_changes() {
            return;
        }
        saving.set(true);
        notice.set(None);
        let update = ActiveOverrideUpdate {
            identity: identity.get_value(),
            scope,
            changes: values.with_value(|entries| {
                entries
                    .iter()
                    .filter_map(|(setting, _, _, original, value)| {
                        (original.get_untracked() != value.get_untracked()).then(|| {
                            OverrideChange {
                                setting: *setting,
                                value: value.get_untracked(),
                            }
                        })
                    })
                    .collect()
            }),
        };
        leptos::task::spawn_local(async move {
            let result = server_fns::save_active_override_config(update).await;
            let (ok, key) = match result {
                Ok(OverrideSettingsOutcome::Applied) => (
                    true,
                    if scope == OverrideScope::Game {
                        Key::GameSettingsApplied
                    } else {
                        Key::SystemSettingsSaved
                    },
                ),
                Ok(OverrideSettingsOutcome::GameChanged) => (false, Key::GameSettingsChanged),
                Ok(OverrideSettingsOutcome::InvalidValue) => (false, Key::GameSettingsInvalid),
                Err(_) => (false, Key::GameSettingsUnavailable),
            };
            // A TV-side game switch may unmount this form during the request.
            if saving.try_set(false).is_some() {
                return;
            }
            if ok {
                settings_saved.set(true);
                values.with_value(|entries| {
                    for (_, inherited, global_default, original, value) in entries {
                        original.set(value.get_untracked());
                        if scope == OverrideScope::Game {
                            inherited.set(*global_default);
                        }
                    }
                });
                if scope == OverrideScope::System && !game_settings_saved.get_untracked() {
                    game_defaults.with_value(|defaults| {
                        values.with_value(|entries| {
                            for (setting, _, global_default, _, value) in entries {
                                if let Some((_, inherited)) =
                                    defaults.iter().find(|(key, _)| key == setting)
                                {
                                    inherited.set(value.get_untracked().or(*global_default));
                                }
                            }
                        });
                    });
                }
            }
            notice.try_set(Some((ok, t(i18n.locale.get_untracked(), key).to_string())));
            if matches!(key, Key::GameSettingsChanged) {
                reload.run(());
            }
        });
    };
    view! {
        <div class="active-game-settings-body">
            <Show when=move || game_settings_saved.get()>
                <p class="active-game-settings-priority">
                    <svg aria-hidden="true" viewBox="0 0 24 24" fill="none" class="active-game-settings-priority-icon">
                        <circle cx="12" cy="12" r="9" />
                        <path d="M12 11v5M12 8h.01" />
                    </svg>
                    <span>{move || t(i18n.locale.get(), if scope == OverrideScope::Game { Key::GameSettingsDefaultNotice } else { Key::SystemSettingsGameOverrideNotice })}</span>
                </p>
            </Show>
            <div class="active-game-overrides-grid">
                {entries.into_iter().map(|(setting,inherited,_,_,value)| view! {
                    <OverrideRow setting inherited value saving />
                }).collect_view()}
            </div>
            <div class="active-game-settings-footer">
                <Show when=move || settings_saved.get()>
                    <RemoveSettingsButton identity scope notice reload settings_saved saving />
                </Show>
                <button type="button" class="form-btn form-btn-secondary active-game-settings-action active-game-settings-reset"
                    class:active-game-settings-reset-saved=move || scope == OverrideScope::System && settings_saved.get()
                    title=move || t(i18n.locale.get(), Key::GameSettingsResetAll)
                    aria-label=move || t(i18n.locale.get(), Key::GameSettingsResetAll)
                    disabled=move || saving.get() || !has_overrides() on:click=reset_all>
                    <SettingsActionIcon kind=SettingsActionIconKind::Reset />
                    <span class="active-game-settings-action-label">{move || t(i18n.locale.get(), Key::GameSettingsResetAll)}</span>
                </button>
                <button type="button" class="form-btn active-game-settings-action active-game-settings-apply"
                    title=move || t(i18n.locale.get(), if scope == OverrideScope::Game { Key::GameSettingsApply } else { Key::SystemSettingsApply })
                    aria-label=move || t(i18n.locale.get(), if scope == OverrideScope::Game { Key::GameSettingsApply } else { Key::SystemSettingsApply })
                    disabled=move || saving.get() || !has_changes() on:click=save>
                    <SettingsActionIcon kind=SettingsActionIconKind::Apply />
                    <span class="active-game-settings-action-label">{move || t(i18n.locale.get(), if scope == OverrideScope::Game { Key::GameSettingsApply } else { Key::SystemSettingsApply })}</span>
                </button>
            </div>
        </div>
    }
}

#[component]
fn RemoveSettingsButton(
    identity: StoredValue<ActiveGameIdentity>,
    scope: OverrideScope,
    notice: RwSignal<Option<(bool, String)>>,
    reload: Callback<()>,
    settings_saved: RwSignal<bool>,
    saving: RwSignal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let confirm_dialog = use_confirm_dialog();
    let (label, confirmation, removed_message, already_removed_message, refresh_message) =
        match scope {
            OverrideScope::Game => (
                Key::RemoveGameSettings,
                Key::RemoveGameSettingsConfirm,
                Key::GameSettingsRemoved,
                Key::GameSettingsAlreadyRemoved,
                Key::GameSettingsRemovedRefresh,
            ),
            OverrideScope::System => (
                Key::RemoveSystemSettings,
                Key::RemoveSystemSettingsConfirm,
                Key::SystemSettingsRemoved,
                Key::SystemSettingsAlreadyRemoved,
                Key::SystemSettingsRemovedRefresh,
            ),
        };
    #[cfg(not(feature = "hydrate"))]
    let _ = refresh_message;
    let remove = Callback::new(move |()| {
        if saving.try_get_untracked() != Some(false) {
            return;
        }
        let Some(identity) = identity.try_get_value() else {
            return;
        };
        saving.try_set(true);
        notice.try_set(None);
        leptos::task::spawn_local(async move {
            let result = server_fns::remove_active_override_settings(identity.clone(), scope).await;
            if saving.try_get_untracked().is_none() {
                return;
            }
            match result {
                Ok(removed) => {
                    settings_saved.set(false);
                    notice.try_set(Some((
                        true,
                        t(
                            i18n.locale.get_untracked(),
                            if removed {
                                removed_message
                            } else {
                                already_removed_message
                            },
                        )
                        .to_string(),
                    )));
                    if removed {
                        #[cfg(feature = "hydrate")]
                        {
                            let mut stable_reads = 0;
                            for _ in 0..15 {
                                TimeoutFuture::new(1_000).await;
                                if saving.try_get_untracked().is_none() {
                                    return;
                                }
                                match server_fns::get_active_override_configs().await {
                                    Ok(config)
                                        if config.game.identity == identity
                                            && !match scope {
                                                OverrideScope::Game => config.game.configured,
                                                OverrideScope::System => config.system.configured,
                                            } =>
                                    {
                                        stable_reads += 1;
                                        if stable_reads == 2 {
                                            saving.try_set(false);
                                            reload.run(());
                                            return;
                                        }
                                    }
                                    _ => stable_reads = 0,
                                }
                            }
                            notice.try_set(Some((
                                false,
                                t(i18n.locale.get_untracked(), refresh_message).to_string(),
                            )));
                        }
                        #[cfg(not(feature = "hydrate"))]
                        reload.run(());
                    } else {
                        reload.run(());
                    }
                    saving.try_set(false);
                }
                Err(error) => {
                    saving.try_set(false);
                    notice.try_set(Some((false, error.to_string())));
                }
            }
        });
    });
    let confirm = move |_| {
        let locale = i18n.locale.get_untracked();
        confirm_dialog.confirm(
            t(locale, label),
            t(locale, confirmation),
            t(locale, Key::RemoveAndRestart),
            true,
            remove,
        );
    };
    view! {
        <button type="button" class="form-btn form-btn-secondary active-game-settings-action active-game-settings-remove"
            title=move || t(i18n.locale.get(), label)
            aria-label=move || t(i18n.locale.get(), label)
            disabled=move || saving.get() || !settings_saved.get() on:click=confirm>
            <SettingsActionIcon kind=SettingsActionIconKind::Remove />
            <span class="active-game-settings-action-label">{move || t(i18n.locale.get(), label)}</span>
        </button>
    }
}

#[derive(Clone, Copy)]
enum SettingsActionIconKind {
    Remove,
    Reset,
    Apply,
}

#[component]
fn SettingsActionIcon(kind: SettingsActionIconKind) -> impl IntoView {
    match kind {
        SettingsActionIconKind::Remove => view! {
            <svg class="active-game-settings-action-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M3 6h18"></path>
                <path d="M8 6V4h8v2"></path>
                <path d="M5 6l1 14h12l1-14"></path>
                <path d="M10 10v6M14 10v6"></path>
            </svg>
        }.into_any(),
        SettingsActionIconKind::Reset => view! {
            <svg class="active-game-settings-action-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M3 12a9 9 0 1 0 3-6.7"></path>
                <path d="M3 3v6h6"></path>
            </svg>
        }.into_any(),
        SettingsActionIconKind::Apply => view! {
            <svg class="active-game-settings-action-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M20 6 9 17l-5-5"></path>
            </svg>
        }.into_any(),
    }
}

#[component]
fn OverrideRow(
    setting: ReplaySetting,
    inherited: RwSignal<Option<i32>>,
    value: RwSignal<Option<i32>>,
    saving: RwSignal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let label_key = setting_label(setting);
    let display = move || match value.get() {
        None => t(i18n.locale.get(), Key::GameSettingsInherit).to_string(),
        Some(value) => setting_value(setting, value, i18n.locale.get()),
    };
    let buttons = [false, true]
        .into_iter()
        .map(|increment| {
            let disabled = Signal::derive(move || {
                saving.get()
                    || setting
                        .step_override(value.get(), inherited.get(), increment)
                        .is_none()
            });
            let label = Signal::derive(move || {
                format!(
                    "{} {}",
                    t(i18n.locale.get(), label_key),
                    if increment { "+" } else { "−" }
                )
            });
            let on_step = Callback::new(move |()| {
                if let Some(next) = setting.step_override(
                    value.get_untracked(),
                    inherited.get_untracked(),
                    increment,
                ) {
                    value.set(next);
                }
            });
            let repeat = matches!(setting, ReplaySetting::MonitorX | ReplaySetting::MonitorY);
            view! { <StepButton label disabled on_step increment repeat /> }
        })
        .collect_view();
    view! {
        <div class="game-override-row">
            <div class="game-override-summary">
                <span class="compact-control-label">{move || t(i18n.locale.get(),label_key)}</span>
                <span class="compact-control-value" title=display>{display}</span>
            </div>
            <div class="game-override-stepper">{buttons}</div>
        </div>
    }
}

fn setting_label(setting: ReplaySetting) -> Key {
    match setting {
        ReplaySetting::Ambiscan => Key::GameSettingsAmbiscan,
        ReplaySetting::IntegerScale => Key::GameSettingsIntegerScale,
        ReplaySetting::VideoFilter => Key::GameSettingsVideoFilter,
        ReplaySetting::Gamma => Key::GameSettingsGamma,
        ReplaySetting::MonitorX => Key::GameSettingsMonitorX,
        ReplaySetting::MonitorY => Key::GameSettingsMonitorY,
        ReplaySetting::Volume => Key::GameSettingsVolume,
    }
}

fn setting_value(setting: ReplaySetting, value: i32, locale: Locale) -> String {
    let key = match (setting, value) {
        (ReplaySetting::Ambiscan | ReplaySetting::IntegerScale | ReplaySetting::VideoFilter, 0) => {
            Some(Key::GameSettingsOff)
        }
        (ReplaySetting::Ambiscan, 1) => Some(Key::GameSettingsAmbiscanFlat),
        (ReplaySetting::Ambiscan, 2) => Some(Key::GameSettingsAmbiscanRadial),
        (ReplaySetting::Ambiscan, 3) => Some(Key::GameSettingsAmbiscanSmart),
        (ReplaySetting::IntegerScale, 1) => Some(Key::GameSettingsScaleVertical),
        (ReplaySetting::IntegerScale, 2) => Some(Key::GameSettingsScaleHorizontal),
        (ReplaySetting::IntegerScale, 3) => Some(Key::GameSettingsScaleFull),
        (ReplaySetting::IntegerScale, 4) => Some(Key::GameSettingsScaleFullOverscan),
        (ReplaySetting::IntegerScale, 5) => Some(Key::GameSettingsScaleVerticalOverscan),
        (ReplaySetting::VideoFilter, 1) => Some(Key::GameSettingsFilterLight),
        (ReplaySetting::VideoFilter, 2) => Some(Key::GameSettingsFilterMedium),
        (ReplaySetting::VideoFilter, 3) => Some(Key::GameSettingsFilterStrong),
        (ReplaySetting::VideoFilter, 4) => Some(Key::GameSettingsFilterBlack),
        _ => None,
    };
    if let Some(key) = key {
        return t(locale, key).to_string();
    }
    match setting {
        ReplaySetting::Volume => format!("{}%", value * 10),
        ReplaySetting::MonitorX | ReplaySetting::MonitorY => format!("{value:+}"),
        _ => setting.encode(value),
    }
}
