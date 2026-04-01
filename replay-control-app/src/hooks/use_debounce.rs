use leptos::prelude::*;

/// Create a debounced signal that updates `delay_ms` milliseconds after `source` changes.
///
/// Returns a new `RwSignal<T>` that trails the source by `delay_ms`. On SSR the
/// returned signal simply mirrors the source's initial value (no timer runs).
///
/// The timer is automatically cleaned up on unmount.
pub fn use_debounced<T: Clone + PartialEq + Send + Sync + 'static>(
    source: RwSignal<T>,
    _delay_ms: i32,
) -> RwSignal<T> {
    let debounced = RwSignal::new(source.get_untracked());

    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        let timer_handle: StoredValue<Option<i32>> = StoredValue::new(None);

        // Track whether the callback has been created so we only leak one Closure
        // for the entire lifetime of this hook (instead of one per keystroke).
        let callback_created: StoredValue<bool> = StoredValue::new(false);
        // The pending value is stored here so the single callback can read it.
        let pending: StoredValue<Option<T>> = StoredValue::new(None);
        // The JS function reference stored as JsValue (Function is !Send, JsValue is !Send too,
        // but StoredValue handles the thread-safety wrapper for us).
        let js_fn: StoredValue<Option<web_sys::js_sys::Function>> = StoredValue::new(None);

        Effect::new(move || {
            let val = source.get();

            // Clear any pending timer.
            if let Some(handle) = timer_handle.get_value()
                && let Some(w) = web_sys::window()
            {
                w.clear_timeout_with_handle(handle);
            }

            // Store the latest value for the callback.
            pending.set_value(Some(val));

            // Create the callback once and store the JS function reference.
            if !callback_created.get_value() {
                let cb = Closure::<dyn Fn()>::new(move || {
                    if let Some(val) = pending.get_value() {
                        debounced.set(val);
                    }
                });
                let func: web_sys::js_sys::Function = cb
                    .as_ref()
                    .unchecked_ref::<web_sys::js_sys::Function>()
                    .clone();
                js_fn.set_value(Some(func));
                cb.forget(); // Leak once, not per keystroke.
                callback_created.set_value(true);
            }

            if let Some(func) = js_fn.get_value()
                && let Some(window) = web_sys::window()
                && let Ok(handle) =
                    window.set_timeout_with_callback_and_timeout_and_arguments_0(&func, _delay_ms)
            {
                timer_handle.set_value(Some(handle));
            }
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
