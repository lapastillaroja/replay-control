//! One-time migration of the legacy local-manuals layout.
//!
//! Local manuals historically lived under retrokit-source folder names
//! (`manuals/snes/`, `manuals/megadrive/`, ...). The layout now follows the
//! system folder names (`manuals/nintendo_snes/`), with only the pooled
//! arcade and pc folders shared across systems. This moves any legacy dirs to
//! the new names on startup so existing users keep their manuals without
//! noticing the change.

use std::fs;
use std::path::Path;

use replay_control_core::systems::SYSTEMS;

/// Move legacy retrokit-named manuals dirs under `manuals_root` to the
/// current per-system layout. Returns the number of legacy dirs migrated
/// (renamed or merged). Idempotent: once a legacy dir is gone the migration
/// is a no-op. Failures on one dir are logged and don't block the others.
pub fn migrate_legacy_manuals_layout(manuals_root: &Path) -> usize {
    if !manuals_root.is_dir() {
        return 0;
    }
    let mut migrated = 0;
    for sys in SYSTEMS {
        let Some(legacy) = sys.retrokit_manuals_folder else {
            continue;
        };
        let target = sys.manuals_folder();
        if legacy == target {
            // Pooled folders (arcade, pc) keep their names.
            continue;
        }
        let legacy_dir = manuals_root.join(legacy);
        // symlink_metadata: a symlinked legacy dir must never be merged
        // through — read_dir would drain the user-managed link target into
        // the new folder. Renaming moves the link itself, which is safe.
        let Ok(meta) = fs::symlink_metadata(&legacy_dir) else {
            continue;
        };
        let is_symlink = meta.file_type().is_symlink();
        // is_dir() follows links: skips plain files, file symlinks, and
        // dangling symlinks alike.
        if !legacy_dir.is_dir() {
            continue;
        }
        let target_dir = manuals_root.join(target);
        let result = if !target_dir.exists() {
            fs::rename(&legacy_dir, &target_dir)
        } else if is_symlink {
            tracing::warn!(
                "manuals layout: {} is a symlink and {} already exists; leaving both in place",
                legacy_dir.display(),
                target_dir.display()
            );
            continue;
        } else {
            merge_into(&legacy_dir, &target_dir)
        };
        match result {
            Ok(()) => {
                tracing::info!(
                    "manuals layout: migrated {} -> {}",
                    legacy_dir.display(),
                    target_dir.display()
                );
                migrated += 1;
            }
            Err(e) => tracing::warn!(
                "manuals layout: failed to migrate {} -> {}: {e}",
                legacy_dir.display(),
                target_dir.display()
            ),
        }
    }
    migrated
}

/// Move every entry of `legacy_dir` into the existing `target_dir`, then
/// remove `legacy_dir` if it ended up empty. Entries that already exist at
/// the target are left in place (never overwrite user files) and logged.
fn merge_into(legacy_dir: &Path, target_dir: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(legacy_dir)? {
        let entry = entry?;
        let destination = target_dir.join(entry.file_name());
        if destination.exists() {
            tracing::warn!(
                "manuals layout: {} already exists, leaving {} behind",
                destination.display(),
                entry.path().display()
            );
            continue;
        }
        fs::rename(entry.path(), &destination)?;
    }
    // Only removable when nothing was left behind by the conflict skip above.
    if fs::read_dir(legacy_dir)?.next().is_none() {
        fs::remove_dir(legacy_dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"pdf").unwrap();
    }

    #[test]
    fn renames_legacy_dir_to_system_folder() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("snes/Super Mario World.pdf"));

        assert_eq!(migrate_legacy_manuals_layout(root.path()), 1);
        assert!(
            root.path()
                .join("nintendo_snes/Super Mario World.pdf")
                .is_file()
        );
        assert!(!root.path().join("snes").exists());
    }

    #[test]
    fn merges_into_existing_target_without_overwriting() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("megadrive/Sonic.pdf"));
        touch(&root.path().join("megadrive/Streets of Rage.pdf"));
        let kept = root.path().join("sega_smd/Sonic.pdf");
        fs::create_dir_all(kept.parent().unwrap()).unwrap();
        fs::write(&kept, b"newer copy").unwrap();

        assert_eq!(migrate_legacy_manuals_layout(root.path()), 1);
        // The conflicting file keeps the target copy and stays in the legacy
        // dir, which therefore survives; the rest moved.
        assert_eq!(fs::read(&kept).unwrap(), b"newer copy");
        assert!(root.path().join("sega_smd/Streets of Rage.pdf").is_file());
        assert!(root.path().join("megadrive/Sonic.pdf").is_file());
    }

    #[test]
    fn pooled_folders_are_left_alone() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("arcade/Street Fighter II.pdf"));
        touch(&root.path().join("pc/Doom.pdf"));

        assert_eq!(migrate_legacy_manuals_layout(root.path()), 0);
        assert!(root.path().join("arcade/Street Fighter II.pdf").is_file());
        assert!(root.path().join("pc/Doom.pdf").is_file());
    }

    #[test]
    fn second_run_is_a_no_op() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("psx/Ridge Racer.pdf"));

        assert_eq!(migrate_legacy_manuals_layout(root.path()), 1);
        assert_eq!(migrate_legacy_manuals_layout(root.path()), 0);
        assert!(root.path().join("sony_psx/Ridge Racer.pdf").is_file());
    }

    #[test]
    fn symlinked_legacy_dir_is_renamed_when_target_missing() {
        let root = tempfile::tempdir().unwrap();
        let shared = tempfile::tempdir().unwrap();
        touch(&shared.path().join("Super Mario World.pdf"));
        fs::create_dir_all(root.path()).unwrap();
        std::os::unix::fs::symlink(shared.path(), root.path().join("snes")).unwrap();

        assert_eq!(migrate_legacy_manuals_layout(root.path()), 1);
        // The link itself moved; the linked-to dir is untouched.
        let renamed = root.path().join("nintendo_snes");
        assert!(fs::symlink_metadata(&renamed).unwrap().is_symlink());
        assert!(renamed.join("Super Mario World.pdf").is_file());
        assert!(shared.path().join("Super Mario World.pdf").is_file());
    }

    #[test]
    fn symlinked_legacy_dir_is_never_merged_through() {
        let root = tempfile::tempdir().unwrap();
        let shared = tempfile::tempdir().unwrap();
        touch(&shared.path().join("Super Mario World.pdf"));
        std::os::unix::fs::symlink(shared.path(), root.path().join("snes")).unwrap();
        fs::create_dir_all(root.path().join("nintendo_snes")).unwrap();

        assert_eq!(migrate_legacy_manuals_layout(root.path()), 0);
        // Nothing drained out of the user-managed link target.
        assert!(shared.path().join("Super Mario World.pdf").is_file());
        assert!(
            fs::symlink_metadata(root.path().join("snes"))
                .unwrap()
                .is_symlink()
        );
        assert!(
            !root
                .path()
                .join("nintendo_snes/Super Mario World.pdf")
                .exists()
        );
    }

    #[test]
    fn missing_root_is_a_no_op() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            migrate_legacy_manuals_layout(&root.path().join("manuals")),
            0
        );
    }
}
