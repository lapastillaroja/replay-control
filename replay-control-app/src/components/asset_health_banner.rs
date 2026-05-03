use leptos::prelude::*;

use replay_control_core::asset_health::AssetHealthIssue;

/// Banner shown on every page when a shipped data asset (catalog, future
/// fonts/themes/etc.) is incompatible with the running binary.
///
/// Reads from a context-provided `RwSignal<Vec<AssetHealthIssue>>` seeded
/// from the `/sse/config` `init` payload and updated via
/// `AssetHealthChanged` events. Parallel to `CorruptionBanner` — the two
/// state machines are distinct (corruption is mutable in-session with rich
/// recovery actions; asset health is set at startup and clears only on
/// restart with no in-app remediation), so the banners coexist rather than
/// share a registry.
///
/// The banner copy is keyed off `kind` for i18n. If a future reporter adds
/// a `kind` not handled here, the issue's `message` field renders as the
/// fallback.
#[component]
pub fn AssetHealthBanner() -> impl IntoView {
    let issues = expect_context::<RwSignal<Vec<AssetHealthIssue>>>();
    view! {
        <Show when=move || !issues.read().is_empty() fallback=|| ()>
            <div class="asset-health-banner">
                {move || {
                    issues
                        .read()
                        .iter()
                        .cloned()
                        .map(|issue| {
                            view! {
                                <div class="asset-health-banner-row">
                                    <span>{copy_for(&issue)}</span>
                                </div>
                            }
                        })
                        .collect::<Vec<_>>()
                }}
            </div>
        </Show>
    }
}

/// Banner copy keyed by `kind`; falls back to the issue's `message` for
/// any kind not yet localised.
fn copy_for(issue: &AssetHealthIssue) -> String {
    match issue.kind.as_str() {
        "schema_too_old" => {
            "Catalog out of date. Reinstall Replay Control to refresh game metadata.".to_string()
        }
        _ => issue.message.clone(),
    }
}
