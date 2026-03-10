# Scroll Restoration & Back-Navigation Analysis

> **Status**: Ready to implement. Recommended approach: global `PageCache` context (see below). Validated by Leptos maintainer.

## Problem Statement

When the user navigates from a long, scrolled page (e.g., a system ROM list at `/games/snes` after loading several pages via infinite scroll) to a detail page (`/games/snes/SuperMario.zip`) and then presses the browser back button, the ROM list page:

1. Loses all scroll position (jumps to top)
2. Loses all infinite-scroll state (only the first page of 100 items is shown)
3. Re-fetches data from the server, potentially showing a loading flash
4. Forces the user to scroll and re-load everything from scratch

This is the single most disruptive UX issue for browse-heavy workflows.

---

## Root Cause Analysis

There are four compounding root causes, all stemming from how Leptos 0.7's router manages component lifecycles.

### 1. Component Remounting (Primary Cause)

**Leptos 0.7's `<Routes>` unmounts the old route's component tree and mounts a fresh one on every navigation, including back-navigation.**

When the URL changes from `/games/snes/SuperMario.zip` back to `/games/snes`, the router:
- Disposes the entire `GameDetailPage` reactive scope
- Creates a brand-new `SystemRomView` component (which creates a brand-new `RomList`)
- All local signals inside `RomList` are re-initialized to their defaults

This means:
- `extra_roms` signal resets to `Vec::new()` (all infinite-scroll pages lost)
- `offset` resets to `PAGE_SIZE` (100)
- `has_more` resets to `false`
- `loading_more` resets to `false`
- The IntersectionObserver for the sentinel is destroyed and recreated

There is **no `KeepAlive` or component-caching mechanism** in Leptos 0.7's router. Every route transition destroys the old component and builds a new one. This is fundamentally different from frameworks like Vue (which has `<keep-alive>`) or React Router v6 (which can preserve component trees with layout routes).

### 2. Resource Re-fetching (Secondary Cause)

`Resource::new` in `RomList` is created fresh on every mount:

```rust
let first_page = Resource::new(
    move || (sys.get_value(), debounced_search.get(), version.get()),
    move |(system, query, _)| {
        server_fns::get_roms_page(system, 0, PAGE_SIZE, query, hh, gf)
    },
);
```

Because this is a new Resource instance (not a cached/global one), it has no previous value. It immediately fires an async fetch to the server. The `<Transition>` wrapper handles this somewhat gracefully (it shows the fallback on first render, then swaps in content), but since there is no previous content to show during the transition (the component was just freshly mounted), the user sees a loading state.

On SSR-hydrated pages, the first render does come from the server, so the initial HTML includes the first page of results. But after hydration, the client-side Resource kicks in and may briefly show the Transition fallback before settling. The key issue remains: only page 0 is loaded; all subsequent infinite-scroll pages are lost.

### 3. Scroll Position Not Preserved

The browser's native `history.scrollRestoration` mechanism (which is `"auto"` by default) attempts to restore scroll position on back-navigation. However, this mechanism relies on the DOM content being available at the same positions when the user navigates back. Because:

- The component remounts from scratch
- Only the first 100 items are rendered (vs. potentially hundreds that were loaded)
- Data loading is async (Suspense/Transition fallback may be shown first)

...the browser either cannot find the target scroll position (the DOM is shorter) or restores to a position that no longer corresponds to the same content. The net result is the page appears to jump to the top.

Additionally, Leptos 0.7 does **not** provide a built-in scroll restoration mechanism. There is no equivalent of React Router's `<ScrollRestoration>` component. The `NavigateOptions` struct has a `scroll` field, but this controls whether to scroll to top on navigation, not whether to save/restore positions.

### 4. The `history.back()` Pattern in GameDetailPage

The game detail page already attempts to use `history.back()` instead of a direct Leptos navigation:

```rust
let _ = history.unwrap().back();
```

This is a good instinct — `history.back()` triggers a `popstate` event which the Leptos router intercepts. However, because the router still remounts the target route's component (cause #1), this does not help with state preservation. The `history.back()` approach correctly avoids pushing a new history entry, but the component lifecycle problem remains.

---

## Upstream Leptos Status

Research into the Leptos GitHub repository confirms that this is a known limitation with no planned framework-level fix.

### Issue #2352 — "Route component without re-render its view" (CLOSED)

- **URL**: https://github.com/leptos-rs/leptos/issues/2352
- A user reported exactly this problem: infinite scroll with pagination resets on back-navigation.
- **Maintainer response (gbj)**: "create a resource that lives in the root of the app, to cache the data, and provide it via context." No `<LeakRoute>` or `<KeepAlive>` component is planned.
- Another user commented (Jan 2025): "sad to see this isn't planned, I do like the idea of `<LeakRoute/>` and can definitely see uses for it like a social media app with a home feed."
- gbj reiterated the same workaround in Jan 2025 — the position has not changed.

### Issue #2666 — "Add scroll prop to `<A>`" (CLOSED)

- **URL**: https://github.com/leptos-rs/leptos/issues/2666
- This issue only controls scroll-to-top behavior on link clicks, **not** back-navigation scroll restoration.
- The `noscroll` attribute was broken in Leptos 0.7, fixed in PR #3333 which added a `scroll` prop to `<A>`.
- **Not relevant to our problem** — it addresses forward-navigation scroll control, not back-nav full page redraw.

### Issue #2164 — Route state lost on back/forward navigation

- **URL**: https://github.com/leptos-rs/leptos/issues/2164
- `NavigateOptions::state` is lost on browser back/forward navigation.
- Related but different — this is about route-level `state` (the `State` field in `NavigateOptions`), not component-level reactive state or scroll position.

### Conclusion

Leptos will **not** add `KeepAlive`, `LeakRoute`, or any route-level component caching mechanism. The maintainer's official recommendation is exactly the approach described in Solution B below: a global cache context provided above `<Router>` that preserves data across navigations. This validates our recommended approach.

---

## Why Infinite Scroll Makes This Worse

Infinite scroll amplifies all four root causes because:

1. **State accumulation**: The user builds up state over time (extra_roms, offset, has_more). A page with simple static content would not suffer from state loss. With infinite scroll, the user may have loaded 5-10 pages (500-1000 items), all stored in local signals that are destroyed on unmount.

2. **Scroll depth**: The more items loaded, the further down the user has scrolled, and the more jarring the reset is. A user 3000px down in a list is teleported back to 0px.

3. **Time cost**: Re-loading all previously loaded pages requires multiple server round-trips. Even if the user manually re-scrolls, each page load takes time (server function call, deserialization, DOM render).

4. **No URL representation**: The current scroll depth and page count are not reflected in the URL. The URL is just `/games/snes` whether the user has loaded 1 page or 10. This means there is no declarative way to restore the full state from the URL alone.

---

## Leptos 0.7 Router Internals

### Route Component Lifecycle

In Leptos 0.7, `<Routes>` works as follows:
- Each `<Route>` has a `view` function that creates the component
- When the matched route changes, the previous route's reactive scope is disposed (all signals, effects, and resources within it are dropped)
- A new reactive scope is created and the new route's `view` function is called
- This happens on both forward navigation and back-navigation (popstate)

There is no concept of route-level component caching. The router does not keep old component trees alive in a hidden state.

### `<Transition>` vs `<Suspense>`

The app correctly uses `<Transition>` in `RomList` (and `SearchPage`), which is the better choice for data reloading because:
- `<Suspense>` completely removes children and shows the fallback when resources are pending
- `<Transition>` keeps the previous children visible while new data loads

However, this distinction only helps when the Resource is **re-triggered within the same component instance** (e.g., when the search query changes). When the component is **remounted**, there are no "previous children" to keep visible — the Transition starts from scratch with the fallback.

### Resource Behavior on Remount

`Resource::new` creates a new resource each time the component mounts. It does not participate in any global cache. The resource will:
1. Run its source function immediately to get the dependency tuple
2. Check if the resource was already resolved during SSR (for hydration)
3. If not hydrating (i.e., client-side navigation), schedule the async fetcher

On back-navigation (which is a client-side navigation, not a fresh SSR page load), step 2 finds no SSR data, so step 3 fires, causing a server function call and a loading flash.

---

## Possible Solutions

### Solution A: URL-Based State Restoration (Medium effort, High impact)

**Encode the pagination state in the URL** so that when the component remounts, it can reconstruct the full state from URL parameters.

**How it works:**
- Track `loaded_pages` (or `offset`) as a URL query param: `/games/snes?p=5` means 5 pages loaded
- On mount, read the `p` param and fetch all pages up to that point (either in parallel or sequentially)
- Store scroll position in `sessionStorage` keyed by URL
- On mount, after data loads, restore scroll position from `sessionStorage`

**Pros:**
- Works with Leptos's current router behavior (no framework modifications)
- URL is shareable and bookmarkable
- Scroll position survives browser refresh too

**Cons:**
- Fetching 5 pages on back-navigation is slower than having them cached
- Need to batch or parallelize the fetches to avoid waterfall
- Adds URL complexity (`?p=5&search=mario`)
- Scroll restoration timing is tricky (must wait for all DOM content to render)

**Feasibility: High** — This can be implemented entirely within the existing app architecture.

### Solution B: Global Signal Store / Context-Based Cache (Medium effort, High impact)

**Move the ROM list state out of the component and into a global reactive store** that survives route transitions.

**How it works:**
- Create a `RomListCache` context provided at the `<App>` level (above the router)
- Store a `HashMap<String, CachedSystemState>` where the key is the system name
- `CachedSystemState` holds: all loaded ROMs, offset, has_more, scroll position
- When `RomList` mounts, check if there's a cache entry for this system
- If yes, restore from cache immediately (no server fetch needed)
- If no, fetch the first page as usual
- On unmount (or periodically), save current state to cache

```rust
struct CachedSystemState {
    roms: Vec<RomEntry>,
    offset: usize,
    has_more: bool,
    scroll_y: f64,
    search_query: String,
}
```

**Pros:**
- Instant back-navigation (no server fetch, no loading flash)
- Preserves exact scroll position and all loaded items
- Cache lives above the router, so it survives route transitions
- Can also cache other pages (favorites, search results)

**Cons:**
- Memory usage: hundreds/thousands of ROM entries kept in memory
- Cache invalidation: need to decide when to expire (on delete/rename, refresh, time-based)
- Scroll restoration still needs manual `window.scrollTo()` after DOM render
- More architectural complexity (global state management)

**Feasibility: High** — Leptos contexts provided above `<Router>` persist across route changes.

### Solution C: Server-Side Resource Caching with `StoredValue` (Low effort, Medium impact)

**Cache the Resource data in a `StoredValue` or similar mechanism outside the component lifecycle.**

This is a lighter version of Solution B. Instead of a full global store, use a module-level or context-level cache just for the most recent server function results.

**How it works:**
- Wrap server function results in a simple cache layer
- On first fetch, store the result
- On subsequent mount (back-navigation), return the cached result synchronously and optionally revalidate in the background

**Pros:**
- Simpler than a full global store
- Reduces loading flashes

**Cons:**
- Does not solve scroll position restoration
- Does not restore infinite-scroll pagination (only caches the first page unless the cache also tracks extra pages)
- `StoredValue` is scoped to the reactive owner — it gets disposed on unmount too. Would need to use a different storage mechanism (e.g., a `once_cell::sync::Lazy` or context above the router)

**Feasibility: Medium** — Limited benefit unless combined with scroll restoration.

### Solution D: Virtual Scrolling (High effort, High impact for large lists)

**Replace infinite scroll with a virtual/windowed scroll** that only renders the visible items plus a buffer.

**How it works:**
- Calculate which items are visible based on scroll position and item height
- Only render those items (plus a buffer above/below)
- Maintain a flat list of all items in memory but only create DOM nodes for visible ones
- On back-navigation, the full list is still lost (same remounting problem), but restoration is much cheaper because all items can be fetched at once (they are just data) and only a small window needs to be rendered

**Pros:**
- Dramatically better performance for very large lists (thousands of items)
- Constant DOM node count regardless of how much the user has scrolled
- Easier to restore state: just need the full data array + scroll offset, rendering is instant

**Cons:**
- Significant implementation effort (virtual scroll is complex, especially with variable-height items like ROM entries with/without thumbnails)
- No established Leptos virtual scroll library exists yet
- Would need to handle: resize, dynamic heights, keyboard navigation, accessibility
- Still needs a state preservation strategy (Solution B or A) for the data

**Feasibility: Low-Medium** — High effort for this specific app. Better suited if there were a ready-made Leptos virtual scroll component.

### Solution E: `bfcache`-Friendly Architecture (Low effort, Low-Medium impact)

**Ensure the page is eligible for the browser's back-forward cache (bfcache).**

Modern browsers can snapshot the entire page state (DOM, JS heap, scroll position) and restore it instantly on back-navigation. However, bfcache is disabled by several factors:
- Open WebSocket/SSE connections
- `Cache-Control: no-store` headers
- `unload` event listeners
- Service workers intercepting navigation

**How it works:**
- Audit the page for bfcache-disqualifying factors
- Ensure the SSR response does not set `Cache-Control: no-store`
- Avoid `unload` listeners (use `pagehide` instead)
- The service worker is minimal (`sw.js` just claims clients) so it should not block bfcache

**Pros:**
- Zero application code changes if bfcache works
- Preserves absolutely everything (DOM, scroll, JS state)

**Cons:**
- bfcache behavior varies across browsers and is not guaranteed
- Leptos's hydration and reactive system may interfere (the framework manipulates DOM on hydration, which might not play well with a frozen page snapshot)
- SPA-style navigation via the Leptos router intercepts `popstate` and does its own routing, which likely bypasses bfcache entirely — bfcache only applies to full-page navigations
- Not a reliable primary strategy

**Feasibility: Low** — SPA routing fundamentally bypasses bfcache. This cannot be the primary solution.

### Solution F: Prevent Component Unmount with Layout Routes (Medium effort, High impact — if supported)

**Use nested/layout routes to keep the list component mounted while showing the detail page.**

**How it works:**
- Instead of `/games/:system` and `/games/:system/:filename` being separate flat routes, make `:filename` a child route of `:system`
- The parent `SystemRomView` stays mounted and visible (or hidden via CSS) while the child `GameDetailPage` renders
- On back-navigation, the child unmounts but the parent (with all its state) remains

**Pros:**
- Solves the root cause directly (component never unmounts)
- All signals, resources, scroll position, loaded items preserved automatically
- No cache management needed

**Cons:**
- Leptos 0.7's `<Route>` does support nested routes with `<Outlet>`, but the layout must accommodate both states (list visible + detail overlay, or list hidden + detail shown)
- Would require significant UI restructuring (detail page as a modal/overlay, or side panel)
- May not fit the current full-page navigation pattern
- The detail page is very content-rich (metadata, videos, actions) — cramming it into an overlay may be awkward

**Feasibility: Medium** — Architecturally sound but requires a UI paradigm shift. Works well for master-detail layouts (common on tablets/desktops) but may feel awkward on mobile.

---

## Recommended Approach

**Combine Solution B (Global Signal Store) with manual scroll restoration**, implemented in phases.

> **Note**: This approach is validated by the Leptos maintainer's official recommendation. In [issue #2352](https://github.com/leptos-rs/leptos/issues/2352), gbj explicitly recommends "a resource that lives in the root of the app, to cache the data, and provide it via context" as the solution to this class of problem. No framework-level alternative is planned.

### Phase 1: Global ROM List Cache (Solves data loss)

1. Create a `RomListCache` struct holding per-system state (loaded ROMs, offset, has_more, search query)
2. Provide it as a context in `App`, above `<Router>`
3. In `RomList`, on mount: check cache. If found, initialize signals from cache instead of defaults. Skip the first-page Resource fetch (or use cache as initial value and revalidate in background)
4. In `RomList`, on every load-more or search change: update the cache entry
5. Invalidate cache on delete/rename (bump a version or clear the entry)

This solves: data loss, re-fetching, loading flash.

### Phase 2: Scroll Position Restoration (Solves scroll jump)

1. In `RomList`, on scroll: debounce-save `window.scrollY` to the cache (or `sessionStorage`)
2. In `RomList`, on mount (after data is restored from cache): schedule a `requestAnimationFrame` to call `window.scrollTo(0, saved_y)` — must wait one frame for DOM to render
3. Disable browser native scroll restoration (`history.scrollRestoration = 'manual'`) to prevent conflicts

This solves: scroll position loss.

### Phase 3: Optional Refinements

- **Cache TTL**: Invalidate cache entries after N minutes or when storage changes are detected
- **Partial re-fetch**: After restoring from cache, do a background revalidation of the first page to pick up any changes (new ROMs added, favorites toggled from another device)
- **Apply the same pattern to other data-heavy pages**: favorites list, search results
- **Consider URL params for pagination** (`?p=5`) as a complement, for the case where the user refreshes the page or shares the URL

### Why This Approach

- **Global store (B) over URL params (A)**: The global store gives instant restoration with zero server round-trips. URL params require re-fetching, which is slower and still produces a loading flash for large page counts. The global store approach is also more natural in a reactive framework.
- **Global store (B) over virtual scrolling (D)**: Virtual scrolling is a performance optimization, not a state preservation solution. It still needs a cache. Better to add it later if performance becomes an issue with very large lists (10,000+ items).
- **Manual scroll restoration over bfcache (E)**: bfcache is unreliable in SPA contexts. Manual restoration is deterministic and works everywhere.
- **Global store (B) over layout routes (F)**: Layout routes would be the cleanest solution architecturally, but they require rethinking the UI paradigm (detail page as overlay or child view). The global store works within the existing full-page navigation pattern with much less disruption.

### Implementation Sketch

```rust
// In a new module, e.g., src/cache.rs

use std::collections::HashMap;
use leptos::prelude::*;

#[derive(Clone, Default)]
pub struct PageCache {
    pub rom_lists: RwSignal<HashMap<String, CachedRomList>>,
}

#[derive(Clone)]
pub struct CachedRomList {
    pub first_page_roms: Vec<RomEntry>,
    pub extra_roms: Vec<RomEntry>,
    pub total: usize,
    pub offset: usize,
    pub has_more: bool,
    pub search_query: String,
    pub scroll_y: f64,
    pub system_display: String,
}

// In App():
//   let cache = PageCache::default();
//   provide_context(cache);

// In RomList, on mount:
//   let cache = expect_context::<PageCache>();
//   if let Some(cached) = cache.rom_lists.read().get(&system) {
//       // Restore from cache: set signals, skip Resource
//   }
```

---

## Pages Affected

| Page | Severity | Why |
|------|----------|-----|
| `/games/:system` (RomList) | Critical | Infinite scroll state, deep scrolling, most common back-nav target |
| `/search` | High | Search results lost, filters reset, but less scroll depth typically |
| `/favorites` | Medium | Single page of data, no infinite scroll, but still remounts |
| `/games` (system grid) | Low | Short page, quick to re-render, minimal scroll depth |
| `/` (home) | Low | Multiple Resources re-fetch but page is short |

---

## Summary

The scroll/state loss on back-navigation is caused by Leptos 0.7's router destroying and recreating route components on every navigation. Since all state (loaded ROMs, scroll position, pagination offset) lives in local signals inside the component, it is lost on unmount. The recommended fix is a global cache context provided above the router that preserves per-system ROM list state across navigations, combined with manual scroll position save/restore. This approach requires no changes to the Leptos framework, works within the existing UI paradigm, and provides instant back-navigation with zero loading flash.
