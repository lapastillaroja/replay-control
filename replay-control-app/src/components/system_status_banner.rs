//! Shared shell for app-wide system-status banners.
//!
//! One visual component for the "message + optional detail + optional action"
//! banner family (storage status, RePlayOS Net Control status, …). Domain
//! components own the state → message mapping and render through this shell so
//! markup and styling live in one place.

use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BannerSeverity {
    Info,
    #[default]
    Warning,
}

impl BannerSeverity {
    fn class(self) -> &'static str {
        match self {
            BannerSeverity::Info => "system-status-banner--info",
            BannerSeverity::Warning => "system-status-banner--warning",
        }
    }
}

/// An optional link rendered after the message ("Set up again" → settings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BannerAction {
    pub href: &'static str,
    pub label: String,
}

/// Renders nothing while `message` is `None`.
#[component]
pub fn SystemStatusBanner(
    #[prop(into)] message: Signal<Option<String>>,
    #[prop(into, optional)] detail: Signal<Option<String>>,
    #[prop(into, optional)] severity: Signal<BannerSeverity>,
    #[prop(into, optional)] action: Signal<Option<BannerAction>>,
) -> impl IntoView {
    view! {
        <Show when=move || message.read().is_some() fallback=|| ()>
            <div class=move || format!("system-status-banner {}", severity.get().class())>
                <div class="system-status-banner-row">
                    <span>{move || message.get().unwrap_or_default()}</span>
                    <Show when=move || detail.read().is_some() fallback=|| ()>
                        <small class="system-status-banner-reason">
                            {move || detail.get().unwrap_or_default()}
                        </small>
                    </Show>
                    <Show when=move || action.read().is_some() fallback=|| ()>
                        <a
                            class="system-status-banner-action"
                            href=move || action.get().map(|a| a.href).unwrap_or_default()
                        >
                            {move || action.get().map(|a| a.label).unwrap_or_default()}
                        </a>
                    </Show>
                </div>
            </div>
        </Show>
    }
}
