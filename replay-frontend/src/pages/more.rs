use leptos::prelude::*;

use crate::api;

#[component]
pub fn MorePage() -> impl IntoView {
    let info = LocalResource::new(|| api::fetch_info());

    view! {
        <div class="page more-page">
            <h2 class="page-title">"More"</h2>

            <div class="menu-list">
                <div class="menu-item">
                    <span class="menu-icon">{"\u{1F4E4}"}</span>
                    <span class="menu-label">"Upload ROMs"</span>
                </div>
                <div class="menu-item">
                    <span class="menu-icon">{"\u{1F4BE}"}</span>
                    <span class="menu-label">"Backup & Restore"</span>
                </div>
                <div class="menu-item">
                    <span class="menu-icon">{"\u{1F4F6}"}</span>
                    <span class="menu-label">"Wi-Fi Configuration"</span>
                </div>
                <div class="menu-item">
                    <span class="menu-icon">{"\u{1F4C1}"}</span>
                    <span class="menu-label">"NFS Share Settings"</span>
                </div>
            </div>

            <h3 class="section-title">"System Info"</h3>
            <Suspense fallback=|| view! { <div class="loading">"Loading..."</div> }>
                {move || {
                    info.get()
                        .map(|result| {
                            match &*result {
                                Ok(info) => {
                                    let kind = info.storage_kind.to_uppercase();
                                    let root = info.storage_root.clone();
                                    let total = format_size(info.disk_total_bytes);
                                    let used = format_size(info.disk_used_bytes);
                                    let avail = format_size(info.disk_available_bytes);
                                    view! {
                                        <div class="info-grid">
                                            <div class="info-row">
                                                <span class="info-label">"Storage"</span>
                                                <span class="info-value">{kind}</span>
                                            </div>
                                            <div class="info-row">
                                                <span class="info-label">"Path"</span>
                                                <span class="info-value">{root}</span>
                                            </div>
                                            <div class="info-row">
                                                <span class="info-label">"Disk Total"</span>
                                                <span class="info-value">{total}</span>
                                            </div>
                                            <div class="info-row">
                                                <span class="info-label">"Disk Used"</span>
                                                <span class="info-value">{used}</span>
                                            </div>
                                            <div class="info-row">
                                                <span class="info-label">"Disk Available"</span>
                                                <span class="info-value">{avail}</span>
                                            </div>
                                        </div>
                                    }
                                        .into_any()
                                }
                                Err(e) => {
                                    view! { <p class="error">{format!("Error: {e}")}</p> }
                                        .into_any()
                                }
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

fn format_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    }
}
