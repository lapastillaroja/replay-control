//! App-shell banner for RePlayOS Net Control states that need user action.
//!
//! Only two states banner (see the integration plan): `Unsupported` (info —
//! the OS predates the API, update it) and `Unauthorized` (warning — the code
//! was reset on the TV, re-onboard). `NotConfigured` never banners: a fresh
//! install or a user who skipped setup shouldn't be nagged on every page —
//! the setup checklist owns that state. `Error` self-recovers and stays quiet.

use leptos::prelude::*;
use replay_control_core::replay_api::ReplayApiStatus;

use crate::components::system_status_banner::{BannerAction, BannerSeverity, SystemStatusBanner};
use crate::i18n::{Key, t, use_i18n};

#[component]
pub fn ReplayApiStatusBanner() -> impl IntoView {
    let i18n = use_i18n();
    let status = expect_context::<RwSignal<ReplayApiStatus>>();

    let message = Signal::derive(move || {
        let locale = i18n.locale.get();
        match status.get() {
            ReplayApiStatus::Unsupported { .. } => {
                Some(t(locale, Key::ReplayApiStatusUnsupported).to_string())
            }
            ReplayApiStatus::Unauthorized => {
                Some(t(locale, Key::ReplayApiStatusUnauthorized).to_string())
            }
            _ => None,
        }
    });
    let severity = Signal::derive(move || match status.get() {
        ReplayApiStatus::Unsupported { .. } => BannerSeverity::Info,
        _ => BannerSeverity::Warning,
    });
    let action = Signal::derive(move || {
        matches!(status.get(), ReplayApiStatus::Unauthorized).then(|| BannerAction {
            href: "/settings/replayos",
            label: t(i18n.locale.get(), Key::ReplayApiSetUpAgain).to_string(),
        })
    });

    view! { <SystemStatusBanner message severity action /> }
}
