use leptos::prelude::*;

use crate::components::system_status_banner::SystemStatusBanner;
use crate::i18n::{Key, Locale, tf, use_i18n};
use crate::types::{StorageStatus, storage_kind_label};

/// App-shell banner for storage states where the gate is still open but the
/// configured target is not being honored. Domain logic only — the markup is
/// the shared [`SystemStatusBanner`] shell.
#[component]
pub fn StorageStatusBanner() -> impl IntoView {
    let i18n = use_i18n();
    let status = expect_context::<RwSignal<StorageStatus>>();
    let message = Signal::derive(move || banner_message(i18n.locale.get(), &status.read()));
    let detail = Signal::derive(move || banner_reason(&status.read()));

    view! { <SystemStatusBanner message detail /> }
}

fn banner_message(locale: Locale, status: &StorageStatus) -> Option<String> {
    match status {
        StorageStatus::Misconfigured {
            wanted,
            current_kind,
            ..
        } => {
            let wanted_label = storage_kind_label(wanted);
            let fallback = current_kind
                .as_deref()
                .filter(|kind| *kind != wanted.as_str())
                .map(|kind| tf(locale, Key::StorageFallback, &[storage_kind_label(kind)]))
                .unwrap_or_default();
            Some(tf(
                locale,
                Key::StorageUnavailable,
                &[wanted_label, fallback.as_str()],
            ))
        }
        StorageStatus::Error { message } => {
            Some(tf(locale, Key::StorageProblem, &[message.as_str()]))
        }
        // ConfigUnavailable is handled by the storage guard redirecting to
        // `/waiting` before any normal page (and thus this banner) mounts —
        // the banner branch is unreachable, so we don't render one here.
        StorageStatus::ConfigUnavailable { .. }
        | StorageStatus::WaitingForMount
        | StorageStatus::Activating
        | StorageStatus::Ready => None,
    }
}

fn banner_reason(status: &StorageStatus) -> Option<String> {
    match status {
        StorageStatus::Misconfigured { reason, .. } => Some(reason.clone()),
        _ => None,
    }
}
