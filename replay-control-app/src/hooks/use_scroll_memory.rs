use leptos::prelude::*;

/// Persist and restore the horizontal `scrollLeft` of a scroll container across
/// client-side navigations — notably browser Back to the home or favorites page.
///
/// The recommendation rows re-mount with fresh DOM when the user navigates Back,
/// resetting `scrollLeft` to 0. This hook remembers the latest `scrollLeft` per
/// row and re-applies it when the row re-mounts.
///
/// `build` returns `(key, signature)`, evaluated on the client only (the hook is
/// a no-op on SSR, so the strings are never allocated during server render):
/// - `key` identifies the row across navigations — keep it stable for the same
///   logical row (we key on route + section identity) and unique between rows.
/// - `signature` describes the row's current content (e.g. a join of the card
///   links). The saved offset is tagged with it and restored **only** when the
///   signature still matches, so a row whose cached content has since regenerated
///   (different cards) starts at 0 instead of restoring a stale offset. Storing
///   the signature alongside the offset (rather than baking it into `key`) keeps
///   the store at one entry per row, so it stays bounded as the short-lived
///   recommendations snapshot regenerates.
pub fn use_scroll_memory(
    node_ref: NodeRef<leptos::html::Div>,
    build: impl FnOnce() -> (String, String),
) {
    #[cfg(feature = "hydrate")]
    {
        use send_wrapper::SendWrapper;
        use std::cell::RefCell;
        use std::collections::HashMap;
        use wasm_bindgen::JsCast;
        use wasm_bindgen::prelude::Closure;

        // key -> (content signature, last scrollLeft). One entry per row, so the
        // store stays bounded even as the cached content regenerates. Shared
        // across every hook instance and surviving SPA navigations within the
        // page session (cleared on a full reload, like the snapshot cache).
        thread_local! {
            static SCROLL_MEMORY: RefCell<HashMap<String, (String, f64)>> =
                RefCell::new(HashMap::new());
        }

        // Build the route-scoped key and content signature once, on the client.
        let (key, signature) = build();

        Effect::new(move || {
            let Some(node) = node_ref.get() else {
                return;
            };
            let Some(window) = web_sys::window() else {
                return;
            };
            let element: web_sys::Element = node.unchecked_into();

            // Restore only when the saved offset belongs to the same content.
            // A regenerated row (different cards -> different signature) starts at
            // 0. Box-art cards have fixed widths, so the scroll extent is stable
            // as soon as the row mounts; we still set `scrollLeft` on the next
            // animation frame because iOS Safari clears it on the mount reflow.
            if let Some((saved_sig, left)) = SCROLL_MEMORY.with(|m| m.borrow().get(&key).cloned())
                && saved_sig == signature
                && left > 0.0
            {
                let el = element.clone();
                let raf = Closure::once_into_js(move || {
                    el.set_scroll_left(left as i32);
                });
                let _ = window.request_animation_frame(raf.unchecked_ref());
            }

            // Save the latest position on every scroll. The (key, signature) pair
            // is allocated only on first touch; later scrolls just update the
            // offset in place — this listener fires at frame rate during a flick.
            let save_key = key.clone();
            let save_sig = signature.clone();
            let save_el = element.clone();
            let on_scroll = Closure::<dyn FnMut(web_sys::Event)>::new(move |_ev| {
                let left = save_el.scroll_left() as f64;
                SCROLL_MEMORY.with(|m| {
                    let mut map = m.borrow_mut();
                    match map.get_mut(&save_key) {
                        Some(slot) => slot.1 = left,
                        None => {
                            map.insert(save_key.clone(), (save_sig.clone(), left));
                        }
                    }
                });
            });
            let opts = web_sys::AddEventListenerOptions::new();
            opts.set_passive(true);
            let _ = element.add_event_listener_with_callback_and_add_event_listener_options(
                "scroll",
                on_scroll.as_ref().unchecked_ref(),
                &opts,
            );

            // Browser types here are `!Send`; wrap for the wasm main thread and
            // drop the closure on cleanup instead of leaking it.
            let teardown = SendWrapper::new((element, on_scroll));
            on_cleanup(move || {
                let (element, on_scroll) = teardown.take();
                let _ = element.remove_event_listener_with_callback(
                    "scroll",
                    on_scroll.as_ref().unchecked_ref(),
                );
                drop(on_scroll);
            });
        });
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = node_ref;
        let _ = build;
    }
}
