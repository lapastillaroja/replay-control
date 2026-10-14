use replay_control_core::replay_settings::{
    ActiveGameIdentity, OverrideScope, VideoSettingsDisplay,
};
use rustix::fd::OwnedFd;
use rustix::fs::{AtFlags, CWD, Mode, OFlags, openat, unlinkat};
use rustix::io::Errno;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;

pub fn remove_override_settings(
    data_root: &Path,
    identity: &ActiveGameIdentity,
    settings_display: VideoSettingsDisplay,
    scope: OverrideScope,
) -> io::Result<bool> {
    if identity.system.is_empty()
        || !identity
            .system
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "invalid system name",
        ));
    }
    let game_file = Path::new(&identity.game_file);
    let filename = game_file
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "invalid game filename"))?;
    if !identity
        .game_file
        .contains(&format!("/roms/{}/", identity.system))
    {
        return Err(io::Error::new(ErrorKind::InvalidInput, "invalid game path"));
    }

    let settings_filename = match scope {
        OverrideScope::Game => format!("{}@{filename}.cfg", identity.system),
        OverrideScope::System => format!("{}.cfg", identity.system),
    };
    let directory_name = match scope {
        OverrideScope::Game => "game",
        OverrideScope::System => "system",
    };
    let mut matches = Vec::new();
    let display_dirs: &[&str] = match settings_display {
        VideoSettingsDisplay::Crt => &["crt"],
        VideoSettingsDisplay::Lcd => &["lcd"],
        VideoSettingsDisplay::Unknown => &["crt", "lcd"],
    };
    for display in display_dirs {
        let Some(directory) = open_settings_dir(data_root, directory_name, display)? else {
            continue;
        };
        match openat(
            &directory,
            settings_filename.as_str(),
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(file) => {
                if !fs::File::from(file).metadata()?.is_file() {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        "unsafe game settings file",
                    ));
                }
                matches.push(directory);
            }
            Err(Errno::NOENT) => {}
            Err(error) => return Err(error.into()),
        }
    }
    match matches.as_slice() {
        [] => Ok(false),
        [directory] => {
            unlinkat(directory, settings_filename.as_str(), AtFlags::empty())?;
            Ok(true)
        }
        _ => Err(io::Error::new(
            ErrorKind::InvalidData,
            "settings exist for both CRT and LCD; select a display mode in RePlayOS before removing them",
        )),
    }
}

fn open_settings_dir(data_root: &Path, scope: &str, display: &str) -> io::Result<Option<OwnedFd>> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = openat(CWD, data_root, flags, Mode::empty())?;
    for component in ["config", "settings", scope, display] {
        directory = match openat(&directory, component, flags, Mode::empty()) {
            Ok(next) => next,
            Err(Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
    }
    Ok(Some(directory))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ActiveGameIdentity {
        ActiveGameIdentity {
            system: "arcade_fbneo".into(),
            game_file: "/media/nfs/roms/arcade_fbneo/Horizontal/00 Clean Romset/punisher.zip"
                .into(),
        }
    }

    #[test]
    fn removes_only_the_matching_game_settings() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("config/settings/game/crt");
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("arcade_fbneo@punisher.zip.cfg");
        let other = directory.join("arcade_fbneo@other.zip.cfg");
        let folder = directory.join("arcade_fbneo@Horizontal@00 Clean Romset.cfg");
        fs::write(&target, "replay_video_monitor_x = \"2\"\n").unwrap();
        fs::write(&other, "keep").unwrap();
        fs::write(&folder, "keep folder settings").unwrap();
        assert!(
            remove_override_settings(
                root.path(),
                &identity(),
                VideoSettingsDisplay::Unknown,
                OverrideScope::Game
            )
            .unwrap()
        );
        assert!(!target.exists());
        assert_eq!(fs::read_to_string(other).unwrap(), "keep");
        assert_eq!(fs::read_to_string(folder).unwrap(), "keep folder settings");
        assert!(
            !remove_override_settings(
                root.path(),
                &identity(),
                VideoSettingsDisplay::Unknown,
                OverrideScope::Game
            )
            .unwrap()
        );
    }

    #[test]
    fn removes_only_the_matching_system_settings() {
        let root = tempfile::tempdir().unwrap();
        let system_dir = root.path().join("config/settings/system/crt");
        let game_dir = root.path().join("config/settings/game/crt");
        fs::create_dir_all(&system_dir).unwrap();
        fs::create_dir_all(&game_dir).unwrap();
        let target = system_dir.join("arcade_fbneo.cfg");
        let other_system = system_dir.join("sega_smd.cfg");
        let game = game_dir.join("arcade_fbneo@punisher.zip.cfg");
        fs::write(&target, "fbneo-dipswitch-punisher-Freeze = 0\n").unwrap();
        fs::write(&other_system, "keep").unwrap();
        fs::write(&game, "keep game settings").unwrap();

        assert!(
            remove_override_settings(
                root.path(),
                &identity(),
                VideoSettingsDisplay::Crt,
                OverrideScope::System,
            )
            .unwrap()
        );
        assert!(!target.exists());
        assert_eq!(fs::read_to_string(other_system).unwrap(), "keep");
        assert_eq!(fs::read_to_string(game).unwrap(), "keep game settings");
    }

    #[test]
    fn refuses_ambiguous_system_display() {
        let root = tempfile::tempdir().unwrap();
        for display in ["crt", "lcd"] {
            let directory = root.path().join("config/settings/system").join(display);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("arcade_fbneo.cfg"), "keep").unwrap();
        }
        assert!(
            remove_override_settings(
                root.path(),
                &identity(),
                VideoSettingsDisplay::Unknown,
                OverrideScope::System,
            )
            .is_err()
        );
        for display in ["crt", "lcd"] {
            assert!(
                root.path()
                    .join("config/settings/system")
                    .join(display)
                    .join("arcade_fbneo.cfg")
                    .exists()
            );
        }
    }

    #[test]
    fn refuses_ambiguous_or_unsafe_game_settings() {
        let root = tempfile::tempdir().unwrap();
        for display in ["crt", "lcd"] {
            let directory = root.path().join("config/settings/game").join(display);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("arcade_fbneo@punisher.zip.cfg"), "keep").unwrap();
        }
        assert!(
            remove_override_settings(
                root.path(),
                &identity(),
                VideoSettingsDisplay::Unknown,
                OverrideScope::Game
            )
            .is_err()
        );
        assert!(
            root.path()
                .join("config/settings/game/crt/arcade_fbneo@punisher.zip.cfg")
                .exists()
        );
        assert!(
            remove_override_settings(
                root.path(),
                &identity(),
                VideoSettingsDisplay::Crt,
                OverrideScope::Game
            )
            .unwrap()
        );
        assert!(
            root.path()
                .join("config/settings/game/lcd/arcade_fbneo@punisher.zip.cfg")
                .exists()
        );
        assert!(
            remove_override_settings(
                root.path(),
                &identity(),
                VideoSettingsDisplay::Lcd,
                OverrideScope::Game
            )
            .unwrap()
        );
        assert!(
            !root
                .path()
                .join("config/settings/game/lcd/arcade_fbneo@punisher.zip.cfg")
                .exists()
        );
        let mut invalid = identity();
        invalid.system = "../arcade_fbneo".into();
        assert!(
            remove_override_settings(
                root.path(),
                &invalid,
                VideoSettingsDisplay::Unknown,
                OverrideScope::Game
            )
            .is_err()
        );
    }

    #[test]
    fn refuses_game_settings_directory_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("config/settings")).unwrap();
        fs::create_dir_all(outside.path().join("crt")).unwrap();
        let target = outside.path().join("crt/arcade_fbneo@punisher.zip.cfg");
        fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("config/settings/game"))
            .unwrap();
        assert!(
            remove_override_settings(
                root.path(),
                &identity(),
                VideoSettingsDisplay::Unknown,
                OverrideScope::Game
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(target).unwrap(), "keep");
    }

    #[test]
    fn refuses_non_regular_game_settings_without_blocking() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("config/settings/game/crt");
        fs::create_dir_all(&directory).unwrap();
        rustix::fs::mkfifoat(
            CWD,
            directory.join("arcade_fbneo@punisher.zip.cfg"),
            Mode::RUSR | Mode::WUSR,
        )
        .unwrap();
        assert!(
            remove_override_settings(
                root.path(),
                &identity(),
                VideoSettingsDisplay::Unknown,
                OverrideScope::Game
            )
            .is_err()
        );
    }
}
