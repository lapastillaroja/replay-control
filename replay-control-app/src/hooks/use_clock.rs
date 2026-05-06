use leptos::prelude::*;

/// Wall-clock signal exposed via context at the App root. Ticks once per
/// second on the client; constant on SSR.
#[derive(Clone, Copy)]
pub struct Clock {
    now_unix: RwSignal<u64>,
}

impl Clock {
    /// Install the clock: spawn the client-side 1 s interval (no-op on SSR)
    /// and return a `Clock` handle suitable for `provide_context`.
    pub fn install() -> Self {
        let now_unix = RwSignal::new(0u64);

        #[cfg(feature = "hydrate")]
        {
            use wasm_bindgen::prelude::*;
            now_unix.set(current_unix_secs());
            let cb = Closure::<dyn FnMut()>::new(move || {
                now_unix.set(current_unix_secs());
            });
            if let Some(window) = web_sys::window() {
                let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    1000,
                );
            }
            cb.forget();
        }

        Self { now_unix }
    }

    pub fn now(&self) -> u64 {
        self.now_unix.get()
    }
}

/// Live elapsed-seconds signal anchored at `started_at_unix_secs` and driven
/// by the app-root `Clock`. Returns `0` when the clock hasn't started yet
/// (SSR, pre-first-tick) or when the client clock is behind the server.
pub fn use_live_elapsed_secs(started_at_unix_secs: u64) -> Signal<u64> {
    let clock = use_context::<Clock>();
    Signal::derive(move || {
        if started_at_unix_secs == 0 {
            return 0;
        }
        clock
            .map(|c| c.now())
            .unwrap_or(0)
            .saturating_sub(started_at_unix_secs)
    })
}

#[cfg(feature = "hydrate")]
fn current_unix_secs() -> u64 {
    js_sys::Date::now() as u64 / 1000
}
