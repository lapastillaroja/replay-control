use leptos::prelude::*;

/// Set up an IntersectionObserver on the given sentinel element ref.
/// Calls `on_intersect` when the element becomes visible (within a 200px margin).
/// Automatically disconnects the observer on cleanup.
///
/// This is a no-op on SSR -- the observer is only created on `hydrate`.
pub fn use_infinite_scroll(
    sentinel_ref: NodeRef<leptos::html::Div>,
    on_intersect: impl Fn() + Send + Sync + 'static,
) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;
        use web_sys::js_sys;

        // Wrap in StoredValue so the closure is Copy and can be shared
        // between the Effect (which re-runs) and the inner Closure.
        let callback = StoredValue::new(on_intersect);

        Effect::new(move || {
            let Some(el) = sentinel_ref.get() else {
                return;
            };

            let cb = Closure::<dyn Fn(js_sys::Array)>::new(move |entries: js_sys::Array| {
                for entry in entries.iter() {
                    if let Ok(entry) = entry.dyn_into::<web_sys::IntersectionObserverEntry>()
                        && entry.is_intersecting()
                    {
                        callback.with_value(|f| f());
                    }
                }
            });

            let opts = web_sys::IntersectionObserverInit::new();
            opts.set_root_margin("200px");

            if let Ok(observer) =
                web_sys::IntersectionObserver::new_with_options(cb.as_ref().unchecked_ref(), &opts)
            {
                let obs_for_cleanup = observer.clone();
                observer.observe(&el);
                on_cleanup(move || {
                    obs_for_cleanup.disconnect();
                });
            }

            cb.forget();
        });
    }

    // Suppress unused variable warning on SSR.
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = sentinel_ref;
        let _ = on_intersect;
    }
}
