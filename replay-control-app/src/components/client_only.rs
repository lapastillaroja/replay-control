use leptos::prelude::*;

/// Mount live device controls after hydration. Their local resources and nested
/// Suspense boundaries must not enter the enclosing page's streamed SSR chunk.
#[component]
pub fn ClientOnly(children: ChildrenFn, #[prop(into)] fallback: ViewFn) -> impl IntoView {
    let hydrated = RwSignal::new(false);
    Effect::new(move |_| hydrated.set(true));
    view! {
        <Show when=move || hydrated.get() fallback=move || fallback.run()>
            {children()}
        </Show>
    }
}
