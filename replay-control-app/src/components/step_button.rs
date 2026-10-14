#[cfg(feature = "hydrate")]
use gloo_timers::callback::{Interval, Timeout};
use leptos::prelude::*;

/// Shared step action: keyboard clicks, or immediate pointer step followed by hold-repeat.
#[component]
pub fn StepButton(
    #[prop(into)] label: Signal<String>,
    #[prop(into)] disabled: Signal<bool>,
    on_step: Callback<()>,
    increment: bool,
    #[prop(optional)] repeat: bool,
) -> impl IntoView {
    #[cfg(feature = "hydrate")]
    let delay = StoredValue::new_local(None::<Timeout>);
    #[cfg(feature = "hydrate")]
    let interval = StoredValue::new_local(None::<Interval>);
    let stop = move || {
        #[cfg(feature = "hydrate")]
        {
            delay.set_value(None);
            interval.set_value(None);
        }
    };
    let step = move || {
        if !disabled.get_untracked() {
            on_step.run(());
        }
    };
    let down = move |event: leptos::ev::PointerEvent| {
        if !repeat || event.button() != 0 || !event.is_primary() || disabled.get_untracked() {
            return;
        }
        stop();
        step();
        #[cfg(feature = "hydrate")]
        delay.set_value(Some(Timeout::new(420, move || {
            interval.set_value(Some(Interval::new(90, step)));
        })));
    };
    // Pointer repeats already perform the first step on down; detail=0 is
    // keyboard/assistive activation and must still work after a cancelled drag.
    let click = move |event: leptos::ev::MouseEvent| {
        if !repeat || event.detail() == 0 {
            step();
        }
    };
    view! {
        <button type="button" class="compact-control compact-control-icon step-control"
            aria-label=move || label.get() disabled=move || disabled.get()
            on:pointerdown=down on:pointerup=move |_| stop()
            on:pointercancel=move |_| stop() on:pointerleave=move |_| stop()
            on:blur=move |_| stop() on:click=click>
            <span aria-hidden="true">{if increment { "+" } else { "\u{2212}" }}</span>
        </button>
    }
}
