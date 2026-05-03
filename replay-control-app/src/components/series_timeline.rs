use leptos::prelude::*;
use leptos_router::components::A;

use crate::i18n::{Key, t, tf, use_i18n};
use crate::server_fns;

/// A single entry in the series timeline.
#[component]
fn TimelineItem(
    entry: server_fns::SeriesTimelineEntry,
    is_last: bool,
) -> impl IntoView {
    let connector_class = if is_last {
        "timeline-connector timeline-connector-last"
    } else {
        "timeline-connector"
    };

    let node_class = if entry.is_current {
        "timeline-node timeline-node-current"
    } else if entry.in_library {
        "timeline-node timeline-node-owned"
    } else {
        "timeline-node timeline-node-missing"
    };

    let title_class = if entry.is_current {
        "timeline-title timeline-title-current"
    } else if entry.in_library {
        "timeline-title"
    } else {
        "timeline-title timeline-title-missing"
    };

    let i18n = use_i18n();
    let in_library = entry.in_library;
    let is_current = entry.is_current;
    let href = entry.href.clone().unwrap_or_default();
    let display_name = entry.display_name.clone();
    let year = entry.year;
    let system_display = entry.system_display.clone();

    let content = if in_library {
        let d = display_name.clone();
        let s = system_display.clone();
        let h = href.clone();
        let current = is_current;
        let i = i18n;
        view! {
            <A href=h attr:class="timeline-link">
                <div class="timeline-content">
                    <div class="timeline-header">
                        <span class=title_class>{d}</span>
                        <Show when=move || year.is_some()>
                            <span class="timeline-year">{year.unwrap()}</span>
                        </Show>
                    </div>
                    <div class="timeline-system">{s}</div>
                    <Show when=move || current>
                        <span class="timeline-current-badge">{move || t(i.locale.get(), Key::SeriesTimelineCurrent)}</span>
                    </Show>
                </div>
            </A>
        }
        .into_any()
    } else {
        let d = display_name.clone();
        let s = system_display.clone();
        let i = i18n;
        view! {
            <div class="timeline-content">
                <div class="timeline-header">
                    <span class=title_class>{d}</span>
                    <Show when=move || year.is_some()>
                        <span class="timeline-year">{year.unwrap()}</span>
                    </Show>
                </div>
                <div class="timeline-system">{s}</div>
                <span class="timeline-not-in-library">{move || t(i.locale.get(), Key::GameDetailNotInLibrary)}</span>
            </div>
        }
        .into_any()
    };

    view! {
        <div class="timeline-item">
            <div class=connector_class></div>
            <div class=node_class></div>
            {content}
        </div>
    }
}

/// Visual series timeline showing all games in a franchise ordered
/// chronologically, with the current game highlighted and library status.
#[component]
pub fn SeriesTimeline(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    series_name: String,
) -> impl IntoView {
    let i18n = use_i18n();
    let has_custom_title = !series_name.is_empty();

    let timeline = Resource::new(
        move || (system.get_value(), rom_filename.get_value()),
        |(sys, fname)| server_fns::get_series_timeline(sys, fname),
    );

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">
                {move || {
                    let locale = i18n.locale.get();
                    if has_custom_title {
                        tf(locale, Key::GameDetailMoreOfSeries, &[&series_name])
                    } else {
                        t(locale, Key::GameDetailMoreInSeries).to_string()
                    }
                }}
            </h2>
            <Suspense fallback=move || view! { <div class="timeline-skeleton"><div class="skeleton-timeline-node"></div><div class="skeleton-timeline-node"></div><div class="skeleton-timeline-node"></div></div> }>
                {move || Suspend::new(async move {
                    let entries = timeline.await?;
                    if entries.is_empty() {
                        return Ok::<_, server_fn::ServerFnError>(view! { <div /> }.into_any());
                    }
                    let count = entries.len();
                    let items: Vec<_> = entries
                        .into_iter()
                        .enumerate()
                        .map(|(i, entry)| {
                            view! {
                                <TimelineItem entry=entry is_last=i == count - 1 />
                            }
                        })
                        .collect();

                    Ok::<_, server_fn::ServerFnError>(view! {
                        <div class="series-timeline">
                            {items}
                        </div>
                    }.into_any())
                })}
            </Suspense>
        </section>
    }
}
