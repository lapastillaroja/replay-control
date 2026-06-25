use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};

/// Shown on a device-only settings sub-page when reached off-device (e.g. by
/// deep link). The menu already disables the entry; this guards direct
/// navigation so the form for a feature that does nothing here isn't presented.
#[component]
pub fn DeviceOnlyNotice() -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <p class="form-hint">{move || t(i18n.locale.get(), Key::SettingsDeviceOnlyDisabled)}</p>
    }
}
