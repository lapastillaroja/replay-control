use leptos::prelude::*;

use crate::components::system_status_banner::SystemStatusBanner;
use crate::types::{StorageStatus, storage_kind_label};

/// App-shell banner for storage states where the gate is still open but the
/// configured target is not being honored. Domain logic only — the markup is
/// the shared [`SystemStatusBanner`] shell.
#[component]
pub fn StorageStatusBanner() -> impl IntoView {
    let status = expect_context::<RwSignal<StorageStatus>>();
    let message = Signal::derive(move || banner_message(&status.read()));
    let detail = Signal::derive(move || banner_reason(&status.read()));

    view! { <SystemStatusBanner message detail /> }
}

fn banner_message(status: &StorageStatus) -> Option<String> {
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
                .map(|kind| format!(" Using {} as fallback.", storage_kind_label(kind)))
                .unwrap_or_default();
            Some(format!(
                "Configured storage {wanted_label} is not available.{fallback} Insert the device or change the storage selection in RePlayOS settings."
            ))
        }
        StorageStatus::Error { message } => Some(format!(
            "Storage problem: {message}. Replay Control is still using the last active storage if one is available."
        )),
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
