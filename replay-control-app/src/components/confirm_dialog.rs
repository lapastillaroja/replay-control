use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::i18n::{Key, t, use_i18n};

#[derive(Clone, Copy)]
pub struct ConfirmDialog {
    request: RwSignal<Option<ConfirmDialogRequest>>,
}

#[derive(Clone)]
struct ConfirmDialogRequest {
    title: String,
    message: String,
    confirm_label: String,
    destructive: bool,
    on_confirm: Callback<()>,
}

pub fn provide_confirm_dialog() -> ConfirmDialog {
    let ctx = ConfirmDialog {
        request: RwSignal::new(None),
    };
    provide_context(ctx);
    #[cfg(target_arch = "wasm32")]
    CLIENT_CONFIRM_DIALOG.with(|cell| cell.set(Some(ctx)));
    ctx
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static CLIENT_CONFIRM_DIALOG: std::cell::Cell<Option<ConfirmDialog>> = const { std::cell::Cell::new(None) };
}

pub fn use_confirm_dialog() -> ConfirmDialog {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(ctx) = use_context::<ConfirmDialog>() {
            return ctx;
        }
        CLIENT_CONFIRM_DIALOG.with(|cell| cell.get()).expect(
            "confirm dialog not initialized: provide_confirm_dialog() must run at the App root",
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    expect_context::<ConfirmDialog>()
}

impl ConfirmDialog {
    pub fn confirm(
        self,
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
        destructive: bool,
        on_confirm: Callback<()>,
    ) {
        self.request.set(Some(ConfirmDialogRequest {
            title: title.into(),
            message: message.into(),
            confirm_label: confirm_label.into(),
            destructive,
            on_confirm,
        }));
    }
}

#[component]
pub fn ConfirmDialogHost() -> impl IntoView {
    let dialog = use_confirm_dialog();
    // Defer the close: a cancel/confirm/overlay click removes this dialog from
    // the DOM, which drops the very click closure that is executing. Doing that
    // synchronously panics wasm ("closure invoked recursively or after being
    // dropped"), so unmount on a microtask once the handler has returned.
    let cancel = Callback::new(move |()| {
        spawn_local(async move { dialog.request.set(None) });
    });

    view! {
        {move || {
            dialog
                .request
                .get()
                .map(|request| view! { <ConfirmDialogBody request on_cancel=cancel /> })
        }}
    }
}

#[component]
fn ConfirmDialogBody(request: ConfirmDialogRequest, on_cancel: Callback<()>) -> impl IntoView {
    let i18n = use_i18n();
    let title = request.title;
    let aria_label = title.clone();
    let message = request.message;
    let confirm_label = request.confirm_label;
    let destructive = request.destructive;
    let on_confirm = request.on_confirm;
    let close_from_backdrop = move |ev: leptos::ev::MouseEvent| {
        let clicked_backdrop = ev
            .target()
            .zip(ev.current_target())
            .is_some_and(|(target, current_target)| target == current_target);

        if clicked_backdrop {
            on_cancel.run(());
        }
    };
    let cancel_from_button = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        on_cancel.run(());
    };
    let confirm = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        // Run the action, then close (close is deferred inside `on_cancel`).
        on_confirm.run(());
        on_cancel.run(());
    };

    view! {
        <div
            class="app-confirm-overlay"
            role="presentation"
            on:click=close_from_backdrop
        >
            <section
                class="app-confirm-dialog"
                role="dialog"
                aria-modal="true"
                aria-label=aria_label
                on:click=|ev| ev.stop_propagation()
            >
                <h2 class="app-confirm-title">{title}</h2>
                <p class="app-confirm-message">{message}</p>
                <div class="app-confirm-actions">
                    <button
                        type="button"
                        class="form-btn form-btn-secondary"
                        on:click=cancel_from_button
                    >
                        {move || t(i18n.locale.get(), Key::CommonCancel)}
                    </button>
                    <button
                        type="button"
                        class="form-btn"
                        class:app-confirm-danger=destructive
                        on:click=confirm
                    >
                        {confirm_label}
                    </button>
                </div>
            </section>
        </div>
    }
}
