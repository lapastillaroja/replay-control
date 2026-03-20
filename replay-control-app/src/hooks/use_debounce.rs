use leptos::prelude::*;

/// Create a debounced signal that updates `delay_ms` milliseconds after `source` changes.
///
/// Returns a new `RwSignal<T>` that trails the source by `delay_ms`. On SSR the
/// returned signal simply mirrors the source's initial value (no timer runs).
///
/// The timer is automatically cleaned up on unmount.
pub fn use_debounced<T: Clone + PartialEq + Send + Sync + 'static>(
    source: RwSignal<T>,
    #[allow(unused_variables)] delay_ms: i32,
) -> RwSignal<T> {
    let debounced = RwSignal::new(source.get_untracked());

    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        let timer_handle: StoredValue<Option<i32>> = StoredValue::new(None);

        Effect::new(move || {
            let val = source.get();

            // Clear any pending timer.
            if let Some(handle) = timer_handle.get_value()
                && let Some(w) = web_sys::window()
            {
                w.clear_timeout_with_handle(handle);
            }

            let cb = Closure::<dyn Fn()>::new(move || {
                debounced.set(val.clone());
            });
            if let Some(window) = web_sys::window()
                && let Ok(handle) =
                    window.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        delay_ms,
                    )
            {
                timer_handle.set_value(Some(handle));
            }
            cb.forget();
        });

        on_cleanup(move || {
            if let Some(handle) = timer_handle.get_value()
                && let Some(w) = web_sys::window()
            {
                w.clear_timeout_with_handle(handle);
            }
        });
    }

    debounced
}
