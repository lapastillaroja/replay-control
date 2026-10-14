use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveStateFile {
    pub slot: u8,
    pub path: PathBuf,
    pub modified_unix_secs: u64,
    pub has_preview: bool,
}

pub fn save_state_preview_path(state_path: &Path) -> PathBuf {
    let mut path = state_path.as_os_str().to_os_string();
    path.push(".png");
    PathBuf::from(path)
}

pub fn list_save_state_files(
    saves_dir: &Path,
    system: &str,
    rom_filename: &str,
) -> Vec<SaveStateFile> {
    if Path::new(system).components().count() != 1
        || !matches!(
            Path::new(system).components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        return Vec::new();
    }
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
                has_preview: save_state_preview_path(&path).is_file(),
                path,
                modified_unix_secs,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_state_preview_next_to_save_file() {
        let temp = tempfile::tempdir().unwrap();
        let saves = temp.path().join("arcade_fbneo");
        std::fs::create_dir(&saves).unwrap();
        std::fs::write(saves.join("punisher.sst2"), b"state").unwrap();
        std::fs::write(saves.join("punisher.sst2.png"), b"preview").unwrap();

        let files = list_save_state_files(temp.path(), "arcade_fbneo", "punisher.zip");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].slot, 2);
        assert!(files[0].has_preview);
        assert!(list_save_state_files(temp.path(), "../outside", "punisher.zip").is_empty());

        std::fs::remove_file(saves.join("punisher.sst2.png")).unwrap();
        assert!(!list_save_state_files(temp.path(), "arcade_fbneo", "punisher.zip")[0].has_preview);
    }
}
