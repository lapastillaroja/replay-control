use leptos::prelude::*;
use replay_control_core::replay_api::{DeviceCapabilities, ReplayApiStatus, ReplayFeature};

use crate::i18n::{Key, Locale, feature_restriction_message, t};
use crate::server_fns::get_replay_capabilities;

#[derive(Clone, Copy)]
pub struct ReplayCapabilities(RwSignal<CapabilityState>);

#[derive(Clone)]
enum CapabilityState {
    NotConnected,
    Loading,
    Unavailable,
    Ready(DeviceCapabilities),
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static CLIENT_CAPABILITIES: std::cell::Cell<Option<ReplayCapabilities>> =
        const { std::cell::Cell::new(None) };
}

/// One request per connected browser, not per game row. Refresh on connection
/// changes and once a minute; the launch handler always checks fresh server-side.
pub fn provide_replay_capabilities(status: RwSignal<ReplayApiStatus>) {
    let capabilities = ReplayCapabilities(RwSignal::new(CapabilityState::Loading));
    provide_context(capabilities);
    #[cfg(target_arch = "wasm32")]
    CLIENT_CAPABILITIES.with(|cell| cell.set(Some(capabilities)));
    let refresh = RwSignal::new(0u64);
    let generation = RwSignal::new(0u64);
    #[cfg(feature = "hydrate")]
    {
        let timer = set_interval_with_handle(
            move || {
                if status.get_untracked().is_active() {
                    refresh.update(|value| *value += 1);
                }
            },
            std::time::Duration::from_secs(60),
        )
        .ok();
        on_cleanup(move || {
            if let Some(timer) = timer {
                timer.clear();
            }
        });
    }
    Effect::new(move |_| {
        refresh.get();
        let connection = status.get();
        generation.update(|value| *value += 1);
        let current = generation.get_untracked();
        if !connection.is_active() {
            capabilities
                .0
                .set(if connection == ReplayApiStatus::NotConfigured {
                    // Standalone mode has no Net Control connection; its simulated
                    // launches retain their existing behavior.
                    CapabilityState::NotConnected
                } else {
                    CapabilityState::Unavailable
                });
            return;
        }
        // Keep a known restriction visible during periodic refreshes.
        if capabilities
            .0
            .with_untracked(|state| matches!(state, CapabilityState::NotConnected))
        {
            capabilities.0.set(CapabilityState::Loading);
        }
        leptos::task::spawn_local(async move {
            let result = match get_replay_capabilities().await {
                Ok(capabilities) => CapabilityState::Ready(capabilities),
                Err(_) => CapabilityState::Unavailable,
            };
            if generation.try_get_untracked() == Some(current) {
                capabilities.0.set(result);
            }
        });
    });
}

pub fn use_replay_capabilities() -> ReplayCapabilities {
    #[cfg(target_arch = "wasm32")]
    return use_context::<ReplayCapabilities>()
        .or_else(|| CLIENT_CAPABILITIES.with(|cell| cell.get()))
        .expect("Replay capabilities must be provided at the App root");
    #[cfg(not(target_arch = "wasm32"))]
    expect_context::<ReplayCapabilities>()
}

impl ReplayCapabilities {
    pub fn launch_restricted_untracked(self, system: &str) -> bool {
        let Some(feature) = ReplayFeature::for_system(system) else {
            return false;
        };
        self.0.with_untracked(|state| match state {
            CapabilityState::NotConnected => false,
            CapabilityState::Ready(capabilities) => capabilities.restriction(feature).is_some(),
            _ => true,
        })
    }

    pub fn launch_restriction(self, system: &str, locale: Locale) -> Option<String> {
        if matches!(&*self.0.read(), CapabilityState::NotConnected) {
            return None;
        }
        self.feature_restriction(ReplayFeature::for_system(system)?, locale)
    }

    pub fn feature_restriction(self, feature: ReplayFeature, locale: Locale) -> Option<String> {
        match &*self.0.read() {
            CapabilityState::Loading => {
                Some(t(locale, Key::ReplayCapabilitiesChecking).to_string())
            }
            CapabilityState::NotConnected | CapabilityState::Unavailable => {
                Some(t(locale, Key::ReplayCapabilitiesUnavailable).to_string())
            }
            CapabilityState::Ready(capabilities) => capabilities
                .restriction(feature)
                .map(|reason| feature_restriction_message(locale, reason)),
        }
    }
}
