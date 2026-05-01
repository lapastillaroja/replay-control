use leptos::prelude::*;

/// One image in the lightbox carousel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LightboxImage {
    pub url: String,
    /// Use nearest-neighbour upscaling — true for pixel-art (screenshots,
    /// title screens), false for photographic content like box art.
    pub pixelated: bool,
}

/// Fullscreen image viewer with prev/next navigation and keyboard support.
#[component]
pub fn ImageLightbox(
    #[prop(into)] images: Signal<Vec<LightboxImage>>,
    current_index: RwSignal<Option<usize>>,
) -> impl IntoView {
    let has_many = move || images.read().len() > 1;

    let advance = move |delta: isize| {
        current_index.update(|idx| {
            let Some(i) = idx else {
                return;
            };
            let n = images.read().len();
            if n == 0 {
                return;
            }
            let next = (*i as isize + delta).rem_euclid(n as isize) as usize;
            *i = next;
        });
    };

    let on_prev = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        advance(-1);
    };

    let on_next = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        advance(1);
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
                // `try_get` returns None if the signal has already been
                // disposed (page unmounted before this listener detached).
                let Some(idx) = current_index.try_get() else {
                    return;
                };
                if idx.is_none() {
                    return;
                }
                match ev.key().as_str() {
                    "Escape" => current_index.set(None),
                    "ArrowLeft" => advance(-1),
                    "ArrowRight" => advance(1),
                    _ => {}
                }
            });
        on_cleanup(move || drop(handle));
    }

    let current = Memo::new(move |_| {
        current_index
            .get()
            .and_then(|i| images.read().get(i).cloned())
    });
    let current_url = move || current.get().map(|img| img.url).unwrap_or_default();
    let current_pixelated = move || current.get().is_some_and(|img| img.pixelated);

    view! {
        <Show when=move || current_index.get().is_some() && !images.read().is_empty()>
            <div class="lightbox-overlay" on:click=on_close>
                <button class="lightbox-close" on:click=on_close_btn>
                    {"\u{2715}"}
                </button>
                <Show when=has_many>
                    <button class="lightbox-nav lightbox-prev" on:click=on_prev>
                        {"\u{2039}"}
                    </button>
                </Show>
                <img
                    class="lightbox-img"
                    class:pixelated=current_pixelated
                    src=current_url
                    alt=""
                />
                <Show when=has_many>
                    <button class="lightbox-nav lightbox-next" on:click=on_next>
                        {"\u{203A}"}
                    </button>
                </Show>
            </div>
        </Show>
    }
}
