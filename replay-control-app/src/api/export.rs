//! Library metadata coverage export.
//!
//! `GET /api/export/library.csv[?system=<folder>]` streams a per-ROM CSV
//! describing, for every metadata field, what the catalog and LaunchBox tiers
//! carry — a gap report for pack/data maintainers. Admin-gated (see
//! `route_required_role` in `api/mod.rs`).
//!
//! With `?system=`, only that system is exported; otherwise every active system
//! is included. The heavy two-tier derivation lives in
//! `replay_control_core_server::coverage_export`; this handler only owns pool
//! access and response framing.

use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::IntoResponse;
use axum::routing::get;
use serde::Deserialize;

use replay_control_core_server::coverage_export::{self, csv_header_line};
use replay_control_core_server::external_metadata;
use replay_control_core_server::library_db::{LibraryDb, SearchFilter};

use super::AppState;

#[derive(Debug, Deserialize)]
struct ExportQuery {
    /// Restrict the export to a single system folder. Absent = whole library.
    system: Option<String>,
}

/// Turn a read-pool result into the value or a 500. `None` is a pool error,
/// `Some(Err)` a query error — either way the export must NOT pretend the data
/// is simply empty (that would misreport a DB failure as "no ROMs / no gaps").
fn require_read<T, E: std::fmt::Display>(
    result: Option<Result<T, E>>,
    context: &str,
) -> Result<T, StatusCode> {
    match result {
        Some(Ok(value)) => Ok(value),
        Some(Err(e)) => {
            tracing::error!("library CSV export: {context} query failed: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
        None => {
            tracing::error!("library CSV export: {context} pool unavailable");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn export_library_csv(
    State(state): State<AppState>,
    Query(params): Query<ExportQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    // Resolve the systems to export: the requested one, or all active systems.
    // A missing OR empty `system` param means "all" — the "All systems" <option>
    // submits `system=` (empty string), which deserializes to Some("").
    let requested = params.system.as_deref().filter(|s| !s.is_empty());
    let systems = match requested {
        Some(system) => vec![system.to_string()],
        None => require_read(
            state.library_reader.read(LibraryDb::active_systems).await,
            "active_systems",
        )?,
    };

    let mut csv = csv_header_line();
    for system in &systems {
        // Full, unfiltered row set for the system (hacks/translations included;
        // they're flagged as columns, not dropped). A read failure here is fatal
        // — dropping a whole system silently would read as "this system has no
        // ROMs" rather than "the query failed".
        let entries = {
            let system = system.clone();
            require_read(
                state
                    .library_reader
                    .read(move |conn| {
                        let filter = SearchFilter::default();
                        LibraryDb::search_game_library(
                            conn,
                            Some(&system),
                            None,
                            &[],
                            &filter,
                            0,
                            usize::MAX,
                        )
                        .map(|(rows, _)| rows)
                    })
                    .await,
                "library read",
            )?
        };
        if entries.is_empty() {
            continue;
        }

        // LaunchBox metadata is optional (often not imported at all), so a miss
        // here legitimately means "no external data" — degrade to blank columns
        // rather than failing the whole export.
        let launchbox = {
            let system = system.clone();
            state
                .external_metadata_reader
                .read(move |conn| {
                    external_metadata::system_launchbox_rows(conn, &system).unwrap_or_default()
                })
                .await
                .unwrap_or_default()
        };

        let media_base = state.storage().rc_dir().join("media").join(system);
        let rows =
            coverage_export::build_system_coverage(system, &entries, &launchbox, &media_base).await;
        for row in &rows {
            csv.push_str(&row.to_csv_line());
        }
    }

    let stamp = coverage_export::export_timestamp();
    let filename = match requested {
        Some(system) => format!("library-metadata-{system}-{stamp}.csv"),
        None => format!("library-metadata-all-{stamp}.csv"),
    };

    Ok((
        [
            (CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        csv,
    ))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/export/library.csv", get(export_library_csv))
}
