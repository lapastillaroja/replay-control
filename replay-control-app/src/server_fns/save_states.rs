use super::*;

#[cfg(feature = "ssr")]
use replay_control_core_server::save_states::list_save_state_files;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveStateSlotStatus {
    pub slot: u8,
    pub modified_unix_secs: Option<u64>,
}

#[server(prefix = "/sfn")]
pub async fn get_save_state_slots(
    system: String,
    rom_filename: String,
) -> Result<Vec<SaveStateSlotStatus>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let saves_dir = state.storage().saves_dir();
    let files = tokio::task::spawn_blocking(move || {
        list_save_state_files(&saves_dir, &system, &rom_filename)
    })
    .await
    .map_err(|e| ServerFnError::new(format!("Save state scan failed: {e}")))?;

    let mut slots: Vec<SaveStateSlotStatus> = (1..=18)
        .map(|slot| SaveStateSlotStatus {
            slot,
            modified_unix_secs: None,
        })
        .collect();
    for file in files {
        if let Some(status) = slots.get_mut(usize::from(file.slot.saturating_sub(1))) {
            status.modified_unix_secs = Some(file.modified_unix_secs);
        }
    }
    Ok(slots)
}
