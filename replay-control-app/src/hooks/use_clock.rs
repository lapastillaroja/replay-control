use leptos::prelude::*;

/// Wall-clock signal exposed via context at the App root. Ticks once per
/// minute on the client (aligned to the wall-clock minute boundary), and
/// stays constant on SSR.
///
/// Per-minute granularity is enough for the only consumers — the now-playing
/// elapsed-time displays — and avoids per-second reactive churn that on iOS
/// Safari can cancel in-flight horizontal momentum scrolls on neighbouring
/// `.scroll-card-row` rows.
#[derive(Clone, Copy)]
pub struct Clock {
    now_unix: RwSignal<u64>,
}

impl Clock {
    /// Install the clock: schedule the client-side per-minute tick (no-op on
    /// SSR) and return a `Clock` handle suitable for `provide_context`.
    pub fn install() -> Self {
        let now_unix = RwSignal::new(0u64);

        #[cfg(feature = "hydrate")]
        {
            use wasm_bindgen::prelude::*;
            let Some(window) = web_sys::window() else {
                return Self { now_unix };
            };

            let window_for_start = window.clone();
            let start_clock = Closure::once_into_js(move || {
                now_unix.set(current_unix_secs());

                // Align the next tick to the wall-clock minute boundary so the
                // displayed elapsed value advances on the "00" second.
                let now_ms = js_sys::Date::now() as u64;
                let delay_to_next_min_ms = (60_000 - (now_ms % 60_000)) as i32;
                let window_for_interval = window_for_start.clone();

                let first_tick = Closure::once_into_js(move || {
                    now_unix.set(current_unix_secs());
                    let recurring = Closure::<dyn FnMut()>::new(move || {
                        now_unix.set(current_unix_secs());
                    });
                    let _ = window_for_interval
                        .set_interval_with_callback_and_timeout_and_arguments_0(
                            recurring.as_ref().unchecked_ref(),
                            60_000,
                        );
                    recurring.forget();
                });

                let _ = window_for_start.set_timeout_with_callback_and_timeout_and_arguments_0(
                    first_tick.unchecked_ref(),
                    delay_to_next_min_ms,
                );
            });
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                start_clock.unchecked_ref(),
                0,
            );
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
