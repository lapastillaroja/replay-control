use leptos::prelude::*;

use crate::server_fns::ScreenshotUrl;

/// Maximum number of capture thumbnails shown before "View all".
pub const INITIAL_CAPTURE_COUNT: usize = 12;

/// Fullscreen lightbox for browsing user captures.
#[component]
pub fn CapturesLightbox(
    screenshots: Vec<ScreenshotUrl>,
    current_index: RwSignal<Option<usize>>,
) -> impl IntoView {
    let count = screenshots.len();
    let screenshots_sv = StoredValue::new(screenshots);

    let on_prev = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        current_index.update(|idx| {
            if let Some(i) = idx {
                *i = if *i == 0 { count - 1 } else { *i - 1 };
            }
        });
    };

    let on_next = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        current_index.update(|idx| {
            if let Some(i) = idx {
                *i = if *i + 1 >= count { 0 } else { *i + 1 };
            }
        });
    };

    let on_close = move |_: leptos::ev::MouseEvent| {
        current_index.set(None);
    };

    let on_close_btn = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        current_index.set(None);
    };

    // Keyboard navigation (hydrate-only)
    #[cfg(feature = "hydrate")]
    {
        use leptos::ev;
        let handle =
            leptos::prelude::window_event_listener(ev::keydown, move |ev: ev::KeyboardEvent| {
                if current_index.get().is_none() {
                    return;
                }
                match ev.key().as_str() {
                    "Escape" => current_index.set(None),
                    "ArrowLeft" => current_index.update(|idx| {
                        if let Some(i) = idx {
                            *i = if *i == 0 { count - 1 } else { *i - 1 };
                        }
                    }),
                    "ArrowRight" => current_index.update(|idx| {
                        if let Some(i) = idx {
                            *i = if *i + 1 >= count { 0 } else { *i + 1 };
                        }
                    }),
                    _ => {}
                }
            });
        on_cleanup(move || drop(handle));
    }

    let current_url = move || {
        current_index
            .get()
            .and_then(|i| screenshots_sv.get_value().get(i).map(|s| s.url.clone()))
    };

    view! {
        <Show when=move || current_index.get().is_some()>
            <div class="lightbox-overlay" on:click=on_close>
                <button class="lightbox-close" on:click=on_close_btn>
                    {"\u{2715}"}
                </button>
                <Show when=move || { count > 1 }>
                    <button class="lightbox-nav lightbox-prev" on:click=on_prev>
                        {"\u{2039}"}
                    </button>
                </Show>
                <img class="lightbox-img" src=current_url alt="Capture" />
                <Show when=move || { count > 1 }>
                    <button class="lightbox-nav lightbox-next" on:click=on_next>
                        {"\u{203A}"}
                    </button>
                </Show>
            </div>
        </Show>
    }
}
