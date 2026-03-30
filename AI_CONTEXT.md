# Code Rules

## General

- Follow best practices for the language and framework in use
- Avoid unnecessary complexity — keep solutions simple and focused
- Write readable code that is easy to maintain
- Prefer small, focused functions and components over large monoliths
- Extract logic from templates — setup above, structure below
- Don't over-engineer: solve the current problem, not hypothetical future ones

## Rust

- Use `clippy` conventions — no unnecessary `clone()`, prefer references where possible
- Prefer `Result` propagation over manual error matching when the caller handles errors
- Use descriptive variable names; avoid single-letter names outside short closures

## Leptos Components

### Structure: setup above, view below

Compute derived signals and closures **above** the `view!` macro. The template should be pure structure:

```rust
#[component]
fn MyComponent() -> impl IntoView {
    // Setup phase: signals, derived values, callbacks
    let count = RwSignal::new(0);
    let label = move || if count.get() % 2 == 0 { "Even" } else { "Odd" };
    let on_click = move |_| count.update(|n| *n += 1);

    // View phase: declarative template
    view! {
        <button on:click=on_click>{label}</button>
    }
}
```

### Use `Suspend` + `ErrorBoundary` for async data

Instead of deeply nested `resource.get().map(|r| match r { Ok/Err })`, use:

```rust
<ErrorBoundary fallback=|errors| view! { <ErrorList errors /> }>
    <Suspense fallback=move || view! { <Loading /> }>
        {move || Suspend::new(async move {
            let data = my_resource.await;
            view! { <DataView data /> }
        })}
    </Suspense>
</ErrorBoundary>
```

- `Suspend::new(async { resource.await })` eliminates `.get().map()` boilerplate
- `ErrorBoundary` catches `Err` from any child rendering a `Result` — no manual match needed
- Use `Transition` instead of `Suspense` when reloading data to avoid flickering

### Signals are Copy — don't clone them

`ReadSignal`, `WriteSignal`, `RwSignal`, and `Memo` are all `Copy`. Move them into multiple closures freely:

```rust
let count = RwSignal::new(0);
// Both closures capture `count` — no clone needed
let increment = move |_| count.update(|n| *n += 1);
let display = move || count.get().to_string();
```

### Use `.read()` instead of `.get()` to avoid cloning values

`.get()` clones the inner value. `.read()` returns a borrow guard:

```rust
// Clones the Vec
let len = move || my_vec.get().len();
// Borrows without cloning
let len = move || my_vec.read().len();
```

### Break large views into child components

Instead of inline `.map()` chains with complex per-item logic inside `view!`, extract a component:

```rust
// Good: each item is its own component
#[component]
fn ItemRow(item: Item) -> impl IntoView {
    view! { <div class="item">{item.name.clone()}</div> }
}
```

### Use `StoredValue` for non-reactive data in closures

When a `String` or other non-`Copy` value needs to be used in multiple closures within `view!`, wrap it in `StoredValue` to make it `Copy`. This eliminates the "clone explosion" pattern:

```rust
// Bad: clone for every closure
let filename = rom.filename.clone();
let filename2 = rom.filename.clone();
let filename3 = rom.filename.clone();

// Good: StoredValue is Copy
let filename = StoredValue::new(rom.filename.clone());
// use filename.get_value() everywhere — no clones needed
```

### Use `<Show>` for conditional rendering

Prefer `<Show>` over `if/else` closures inside `view!`:

```rust
<Show when=move || is_active.get() fallback=|| view! { <Inactive /> }>
    <Active />
</Show>
```

### Use `#[prop(into)]` for flexible component APIs

```rust
#[component]
fn Badge(#[prop(into)] label: Signal<String>) -> impl IntoView { ... }
```

### Use `bind:` for two-way binding

Replaces `prop:value` + `on:input` boilerplate:

```rust
let name = RwSignal::new("".to_string());
view! { <input type="text" bind:value=name /> }
```
