use crate::components::client_only::ClientOnly;
use crate::components::status_message::StatusMessage;
use crate::components::step_button::StepButton;
use crate::i18n::{Key, Locale, t, use_i18n};
use crate::server_fns;
use leptos::prelude::*;
use replay_control_core::replay_settings::{
    ActiveGameConfig, GameOverride, GameSetting, GameSettingsOutcome,
};

#[component]
pub fn ActiveGameSettings(system: String, rom_path: String) -> impl IntoView {
    let i18n = use_i18n();
    let system = StoredValue::new(system);
    let rom_path = StoredValue::new(rom_path);
    let refresh = RwSignal::new(0u64);
    let notice = RwSignal::new(None::<(bool, String)>);
    let config = LocalResource::new(move || {
        refresh.get();
        server_fns::get_active_game_config()
    });
    let reload = Callback::new(move |()| refresh.update(|value| *value += 1));
    view! {
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), Key::GameSettingsTitle)}</h2>
            <p class="form-hint">{move || t(i18n.locale.get(), Key::GameSettingsHint)}</p>
            <StatusMessage status=notice />
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
                        if !data.identity.matches_library_game(&system.get_value(), &rom_path.get_value()) {
                            return Ok::<_, server_fn::ServerFnError>(view! {
                                <p class="form-hint">{move || t(i18n.locale.get(), Key::GameSettingsChanged)}</p>
                            }.into_any());
                        }
                        Ok(view! { <ActiveGameSettingsForm config=data reload notice /> }.into_any())
                    })}
                </Suspense>
            </ErrorBoundary>
            </ClientOnly>
        </section>
    }
}

#[component]
fn ActiveGameSettingsForm(
    config: ActiveGameConfig,
    reload: Callback<()>,
    notice: RwSignal<Option<(bool, String)>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let identity = StoredValue::new(config.identity);
    let entries: Vec<_> = config
        .overrides
        .into_iter()
        .map(|entry| (entry.setting, entry.inherited, RwSignal::new(entry.value)))
        .collect();
    let values = StoredValue::new(entries.clone());
    let saving = RwSignal::new(false);
    let save = move |_| {
        if saving.get_untracked() {
            return;
        }
        saving.set(true);
        notice.set(None);
        let config = ActiveGameConfig {
            identity: identity.get_value(),
            overrides: values.with_value(|entries| {
                entries
                    .iter()
                    .map(|(setting, inherited, value)| GameOverride {
                        setting: *setting,
                        inherited: *inherited,
                        value: value.get_untracked(),
                    })
                    .collect()
            }),
        };
        leptos::task::spawn_local(async move {
            let result = server_fns::save_active_game_config(config).await;
            let (ok, key) = match result {
                Ok(GameSettingsOutcome::Applied) => (true, Key::GameSettingsApplied),
                Ok(GameSettingsOutcome::GameChanged) => (false, Key::GameSettingsChanged),
                Ok(GameSettingsOutcome::InvalidValue) => (false, Key::GameSettingsInvalid),
                Err(_) => (false, Key::GameSettingsUnavailable),
            };
            // A TV-side game switch may unmount this form during the request.
            if saving.try_set(false).is_some() {
                return;
            }
            notice.try_set(Some((ok, t(i18n.locale.get_untracked(), key).to_string())));
            if matches!(key, Key::GameSettingsChanged) {
                reload.run(());
            }
        });
    };
    view! {
        <div class="active-game-settings-panel">
            <div class="active-game-overrides-grid">
                {entries.into_iter().map(|(setting,inherited,value)| view! {
                    <OverrideRow setting inherited value saving />
                }).collect_view()}
            </div>
            <div class="active-game-settings-footer">
                <button type="button" class="compact-control active-game-settings-apply"
                    disabled=move || saving.get() on:click=save>
                    {move || t(i18n.locale.get(),Key::GameSettingsApply)}
                </button>
            </div>
        </div>
    }
}

#[component]
fn OverrideRow(
    setting: GameSetting,
    inherited: Option<i32>,
    value: RwSignal<Option<i32>>,
    saving: RwSignal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let label_key = setting_label(setting);
    let current = move || value.get().or(inherited);
    let display = move || match value.get() {
        None => t(i18n.locale.get(), Key::GameSettingsInherit).to_string(),
        Some(value) => setting_value(setting, value, i18n.locale.get()),
    };
    let buttons = [false, true]
        .into_iter()
        .map(|increment| {
            let disabled = Signal::derive(move || {
                saving.get()
                    || current().is_none_or(|number| setting.adjust(number, increment) == number)
            });
            let label = Signal::derive(move || {
                format!(
                    "{} {}",
                    t(i18n.locale.get(), label_key),
                    if increment { "+" } else { "−" }
                )
            });
            let on_step = Callback::new(move |()| {
                if let Some(number) = value.get_untracked().or(inherited) {
                    value.set(Some(setting.adjust(number, increment)));
                }
            });
            let repeat = matches!(setting, GameSetting::MonitorX | GameSetting::MonitorY);
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

fn setting_label(setting: GameSetting) -> Key {
    match setting {
        GameSetting::Ambiscan => Key::GameSettingsAmbiscan,
        GameSetting::IntegerScale => Key::GameSettingsIntegerScale,
        GameSetting::VideoFilter => Key::GameSettingsVideoFilter,
        GameSetting::Gamma => Key::GameSettingsGamma,
        GameSetting::MonitorX => Key::GameSettingsMonitorX,
        GameSetting::MonitorY => Key::GameSettingsMonitorY,
        GameSetting::Volume => Key::GameSettingsVolume,
    }
}

fn setting_value(setting: GameSetting, value: i32, locale: Locale) -> String {
    let key = match (setting, value) {
        (GameSetting::Ambiscan | GameSetting::IntegerScale | GameSetting::VideoFilter, 0) => {
            Some(Key::GameSettingsOff)
        }
        (GameSetting::Ambiscan, 1) => Some(Key::GameSettingsAmbiscanFlat),
        (GameSetting::Ambiscan, 2) => Some(Key::GameSettingsAmbiscanRadial),
        (GameSetting::Ambiscan, 3) => Some(Key::GameSettingsAmbiscanSmart),
        (GameSetting::IntegerScale, 1) => Some(Key::GameSettingsScaleVertical),
        (GameSetting::IntegerScale, 2) => Some(Key::GameSettingsScaleHorizontal),
        (GameSetting::IntegerScale, 3) => Some(Key::GameSettingsScaleFull),
        (GameSetting::IntegerScale, 4) => Some(Key::GameSettingsScaleFullOverscan),
        (GameSetting::IntegerScale, 5) => Some(Key::GameSettingsScaleVerticalOverscan),
        (GameSetting::VideoFilter, 1) => Some(Key::GameSettingsFilterLight),
        (GameSetting::VideoFilter, 2) => Some(Key::GameSettingsFilterMedium),
        (GameSetting::VideoFilter, 3) => Some(Key::GameSettingsFilterStrong),
        (GameSetting::VideoFilter, 4) => Some(Key::GameSettingsFilterBlack),
        _ => None,
    };
    if let Some(key) = key {
        return t(locale, key).to_string();
    }
    match setting {
        GameSetting::Volume => format!("{}%", value * 10),
        GameSetting::MonitorX | GameSetting::MonitorY => format!("{value:+}"),
        _ => setting.encode(value),
    }
}
