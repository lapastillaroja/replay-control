use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveStateFile {
    pub slot: u8,
    pub path: PathBuf,
    pub modified_unix_secs: u64,
}

pub fn list_save_state_files(
    saves_dir: &Path,
    system: &str,
    rom_filename: &str,
) -> Vec<SaveStateFile> {
    let Some(stem) = Path::new(rom_filename)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
    else {
        return Vec::new();
    };

    let system_saves_dir = saves_dir.join(system);
    (1..=18)
        .filter_map(|slot| {
            let path = system_saves_dir.join(format!("{stem}.sst{slot}"));
            let modified_unix_secs = path
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs())?;
            Some(SaveStateFile {
                slot,
                path,
                modified_unix_secs,
            })
        })
        .collect()
}
