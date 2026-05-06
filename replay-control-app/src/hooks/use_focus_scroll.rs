use leptos::html::ElementType;
use leptos::prelude::*;

/// Keep a mounted DOM node scrolled into view while `should_focus` is true.
///
/// A one-shot `scrollIntoView` isn't enough on this page: cover-art images
/// and lazily-rendered sections finish laying out **after** the initial
/// scroll, pushing the target down while the user stays at the old Y. To
/// stay anchored, the hook also installs a `ResizeObserver` on `<body>` and
/// re-issues the scroll on each resize — until the user moves the page
/// themselves (wheel/touch/keydown), at which point re-scrolling stops
/// until `should_focus` cycles false → true again.
pub fn use_focus_scroll<E>(
    node_ref: NodeRef<E>,
    should_focus: impl Fn() -> bool + Send + Sync + 'static,
) where
    E: ElementType,
    E::Output: wasm_bindgen::JsCast + Clone + 'static,
{
    #[cfg(feature = "hydrate")]
    {
        use send_wrapper::SendWrapper;
        use std::cell::Cell;
        use std::rc::Rc;
        use wasm_bindgen::JsCast;
        use wasm_bindgen::prelude::Closure;

        Effect::new(move || {
            if !should_focus() {
                return;
            }
            let Some(node) = node_ref.get() else {
                return;
            };
            let Some(window) = web_sys::window() else {
                return;
            };
            let Some(document) = window.document() else {
                return;
            };
            let Some(body) = document.body() else {
                return;
            };

            let element: web_sys::Element = node.unchecked_into();

            // Tracks whether a programmatic scroll is in flight so the global
            // scroll listener doesn't mistake our own scrollTo for a user
            // override. Set just before scheduling rAF, cleared on the next
            // task tick after the scroll has been issued.
            let scrolling_programmatically = Rc::new(Cell::new(false));
            let user_overrode = Rc::new(Cell::new(false));

            // `scroll_now` may fire many times over the page lifetime (on every
            // ResizeObserver entry). Both rAF and setTimeout are one-shot, so we
            // use `Closure::once_into_js` — wasm-bindgen drops the closure once
            // JS releases it, instead of leaking one wrapper per call.
            let scroll_now = {
                let element = element.clone();
                let window = window.clone();
                let scrolling_programmatically = scrolling_programmatically.clone();
                let user_overrode = user_overrode.clone();
                move || {
                    if user_overrode.get() {
                        return;
                    }
                    let opts = web_sys::ScrollIntoViewOptions::new();
                    opts.set_behavior(web_sys::ScrollBehavior::Smooth);
                    opts.set_block(web_sys::ScrollLogicalPosition::Start);

                    scrolling_programmatically.set(true);
                    let element = element.clone();
                    let scrolling_flag = scrolling_programmatically.clone();
                    let raf = Closure::once_into_js(move || {
                        element.scroll_into_view_with_scroll_into_view_options(&opts);
                    });
                    let _ = window.request_animation_frame(raf.unchecked_ref());

                    let clear = Closure::once_into_js(move || {
                        scrolling_flag.set(false);
                    });
                    let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                        clear.unchecked_ref(),
                        300,
                    );
                }
            };

            // Initial scroll attempt. If the layout is already stable this is
            // the only one we need; otherwise the ResizeObserver catches up.
            scroll_now();

            // ResizeObserver on the body: re-scroll whenever the document
            // grows or shrinks (image decode, section async-resolves, etc.).
            // The callback closure is kept in `teardown` (below) so it's
            // dropped on cleanup along with the observer.
            let scroll_for_observer = scroll_now.clone();
            let observer_cb = Closure::<dyn FnMut(js_sys::Array)>::new(move |_entries| {
                scroll_for_observer();
            });
            let observer = match web_sys::ResizeObserver::new(observer_cb.as_ref().unchecked_ref())
            {
                Ok(o) => o,
                Err(_) => return,
            };
            observer.observe(body.as_ref());

            // User-scroll detector. We watch passive scroll events on the
            // window; if scrollY moves and we weren't the ones that moved it,
            // mark as overridden and stop re-scrolling.
            let scroll_listener = {
                let scrolling_flag = scrolling_programmatically.clone();
                let user_overrode = user_overrode.clone();
                Closure::<dyn FnMut(web_sys::Event)>::new(move |_ev| {
                    if scrolling_flag.get() {
                        return;
                    }
                    user_overrode.set(true);
                })
            };
            let listener_opts = web_sys::AddEventListenerOptions::new();
            listener_opts.set_passive(true);
            let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
                "wheel",
                scroll_listener.as_ref().unchecked_ref(),
                &listener_opts,
            );
            let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
                "touchstart",
                scroll_listener.as_ref().unchecked_ref(),
                &listener_opts,
            );
            let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
                "keydown",
                scroll_listener.as_ref().unchecked_ref(),
                &listener_opts,
            );

            // Tear down when `should_focus` flips back to false (Effect re-run)
            // or the component disposes. Browser types here are `!Send`, so
            // we wrap in `SendWrapper`, which is only ever touched on the
            // wasm main thread. Both Closures are stored in the tuple so they
            // drop with it instead of leaking via `forget`.
            let teardown = SendWrapper::new((window, observer, observer_cb, scroll_listener));
            on_cleanup(move || {
                let (window, observer, observer_cb, scroll_listener) = teardown.take();
                observer.disconnect();
                let _ = window.remove_event_listener_with_callback(
                    "wheel",
                    scroll_listener.as_ref().unchecked_ref(),
                );
                let _ = window.remove_event_listener_with_callback(
                    "touchstart",
                    scroll_listener.as_ref().unchecked_ref(),
                );
                let _ = window.remove_event_listener_with_callback(
                    "keydown",
                    scroll_listener.as_ref().unchecked_ref(),
                );
                drop(observer_cb);
                drop(scroll_listener);
            });
        });
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = node_ref;
        let _ = should_focus;
    }
}
