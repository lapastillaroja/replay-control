use leptos::prelude::*;

use crate::i18n::{I18nContext, Key, t, use_i18n};

/// Shown on a device-only settings sub-page when reached off-device (e.g. by
/// deep link). The menu already disables the entry; this guards direct
/// navigation so the form for a feature that does nothing here isn't presented.
#[component]
pub fn DeviceOnlyNotice(#[prop(optional)] i18n: Option<I18nContext>) -> impl IntoView {
    let i18n = i18n.unwrap_or_else(use_i18n);
    view! {
        <p class="form-hint">{move || t(i18n.locale.get(), Key::SettingsDeviceOnlyDisabled)}</p>
    }
}
