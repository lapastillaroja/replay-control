//! Managed game overrides shared by the UI and native API validation.
use crate::replay_api::{ReplayConfigSnapshot, StatusResponse};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplaySetting {
    Ambiscan,
    IntegerScale,
    VideoFilter,
    Gamma,
    MonitorX,
    MonitorY,
    Volume,
}

impl ReplaySetting {
    pub const ALL: [Self; 7] = [
        Self::Ambiscan,
        Self::IntegerScale,
        Self::VideoFilter,
        Self::Gamma,
        Self::MonitorX,
        Self::MonitorY,
        Self::Volume,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Ambiscan => "replay_video_ambiscan",
            Self::IntegerScale => "replay_video_integer_scale",
            Self::VideoFilter => "replay_video_filter",
            Self::Gamma => "replay_video_gamma",
            Self::MonitorX => "replay_video_monitor_x",
            Self::MonitorY => "replay_video_monitor_y",
            Self::Volume => "replay_audio_system_volume",
        }
    }
    pub fn bounds(self) -> (i32, i32, i32) {
        match self {
            Self::Ambiscan => (0, 3, 1),
            Self::IntegerScale => (0, 5, 1),
            Self::VideoFilter => (0, 4, 1),
            Self::Gamma => (5, 15, 1),
            Self::MonitorX | Self::MonitorY => (-64, 64, 2),
            Self::Volume => (0, 10, 1),
        }
    }

    /// Global replay.cfg names omit the override's `replay_` prefix.
    pub fn global_key(self) -> &'static str {
        match self {
            Self::Ambiscan => "video_ambiscan",
            Self::IntegerScale => "video_integer_scale",
            Self::VideoFilter => "video_filter",
            Self::Gamma => "video_gamma",
            Self::MonitorX => "video_monitor_x",
            Self::MonitorY => "video_monitor_y",
            Self::Volume => "audio_system_volume",
        }
    }
    pub fn parse(self, value: &str) -> Option<i32> {
        let number = if self == Self::Gamma {
            let (whole, fraction) = value.split_once('.').unwrap_or((value, "0"));
            if fraction.len() != 1 {
                return None;
            }
            whole
                .parse::<i32>()
                .ok()?
                .checked_mul(10)?
                .checked_add(fraction.parse::<i32>().ok()?)?
        } else {
            value.parse().ok()?
        };
        let (min, max, step) = self.bounds();
        (number >= min && number <= max && (number - min) % step == 0).then_some(number)
    }
    pub fn encode(self, value: i32) -> String {
        if self == Self::Gamma {
            format!("{}.{}", value / 10, value % 10)
        } else {
            value.to_string()
        }
    }
    pub fn adjust(self, value: i32, increment: bool) -> i32 {
        let (min, max, step) = self.bounds();
        (value + if increment { step } else { -step }).clamp(min, max)
    }

    pub fn step_override(
        self,
        value: Option<i32>,
        inherited: Option<i32>,
        increment: bool,
    ) -> Option<Option<i32>> {
        if self != Self::Volume {
            let current = value.or(inherited)?;
            let next = self.adjust(current, increment);
            return (next != current).then_some(Some(next));
        }

        let (min, max, _) = self.bounds();
        let default_position = max - min + 1;
        let position = value.map_or(default_position, |number| number - min);
        let next_position = position + if increment { 1 } else { -1 };
        if !(0..=default_position).contains(&next_position) {
            return None;
        }
        Some((next_position != default_position).then_some(next_position + min))
    }

    pub fn available_in_video_mode(self, mode: Option<&str>) -> bool {
        VideoSettingsDisplay::from_video_mode(mode) != VideoSettingsDisplay::Crt
            || !matches!(self, Self::IntegerScale | Self::VideoFilter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoSettingsDisplay {
    Crt,
    Lcd,
    Unknown,
}

impl VideoSettingsDisplay {
    pub fn from_video_mode(mode: Option<&str>) -> Self {
        match mode {
            Some("1") => Self::Crt,
            Some("3" | "4" | "5" | "6" | "7" | "8" | "9" | "11" | "12" | "13") => Self::Lcd,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveGameIdentity {
    pub system: String,
    pub game_file: String,
}
impl ActiveGameIdentity {
    /// Library paths start at /roms; device paths also include the storage mount.
    /// Keep subdirectories: the detail-page URL intentionally uses only a filename.
    pub fn matches_library_game(&self, system: &str, rom_path: &str) -> bool {
        self.system == system
            && rom_path.starts_with(&format!("/roms/{system}/"))
            && self.game_file.ends_with(rom_path)
    }

    pub fn from_status(status: &StatusResponse) -> Option<Self> {
        let system = status.system.as_ref().filter(|value| !value.is_empty())?;
        let game_file = status
            .game_file
            .as_ref()
            .filter(|value| !value.is_empty())?;
        Some(Self {
            system: system.clone(),
            game_file: game_file.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingOverride {
    pub setting: ReplaySetting,
    /// None uses RePlayOS's global default; numbers use the setting's units.
    pub value: Option<i32>,
    pub inherited: Option<i32>,
    pub global_default: Option<i32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverrideScope {
    System,
    Game,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveOverrideConfig {
    pub identity: ActiveGameIdentity,
    pub scope: OverrideScope,
    pub configured: bool,
    pub overrides: Vec<SettingOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideChange {
    pub setting: ReplaySetting,
    pub value: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveOverrideUpdate {
    pub identity: ActiveGameIdentity,
    pub scope: OverrideScope,
    pub changes: Vec<OverrideChange>,
}

impl ActiveOverrideUpdate {
    pub fn is_valid(&self) -> bool {
        !self.changes.is_empty()
            && self.changes.len() <= ReplaySetting::ALL.len()
            && self.changes.iter().all(|change| {
                ReplaySetting::ALL.contains(&change.setting)
                    && change.value.is_none_or(|value| {
                        change.setting.parse(&change.setting.encode(value)) == Some(value)
                    })
            })
            && ReplaySetting::ALL.iter().all(|setting| {
                self.changes
                    .iter()
                    .filter(|change| change.setting == *setting)
                    .count()
                    <= 1
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveOverrideConfigs {
    pub game: ActiveOverrideConfig,
    pub system: ActiveOverrideConfig,
}

impl ActiveOverrideConfig {
    pub fn from_snapshots(
        identity: ActiveGameIdentity,
        scope: OverrideScope,
        game: &ReplayConfigSnapshot,
        core: &ReplayConfigSnapshot,
        global: &ReplayConfigSnapshot,
    ) -> Self {
        let video_mode = global.get_value_string("video_mode");
        let overrides = ReplaySetting::ALL
            .into_iter()
            .filter(|setting| setting.available_in_video_mode(video_mode.as_deref()))
            .map(|setting| {
                let read = |snapshot: &ReplayConfigSnapshot| {
                    snapshot
                        .get_value_string(setting.key())
                        .and_then(|value| setting.parse(&value))
                };
                let global_default = global
                    .get_value_string(setting.global_key())
                    .and_then(|value| setting.parse(&value));
                SettingOverride {
                    setting,
                    value: read(match scope {
                        OverrideScope::Game => game,
                        OverrideScope::System => core,
                    }),
                    inherited: (scope == OverrideScope::Game && !game.configured)
                        .then(|| read(core))
                        .flatten()
                        .or(global_default),
                    global_default,
                }
            })
            .collect();
        Self {
            identity,
            scope,
            configured: match scope {
                OverrideScope::Game => game.configured,
                OverrideScope::System => core.configured,
            },
            overrides,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverrideSettingsOutcome {
    Applied,
    GameChanged,
    InvalidValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsOutcome {
    Applied,
    Restarting,
    InvalidMode,
    InvalidAddress,
    InvalidPort,
    InvalidToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkPlayConfig {
    pub mode: String,
    pub address: String,
    pub port: u16,
    pub lan_discovery: bool,
}

impl LinkPlayConfig {
    pub fn from_snapshot(snapshot: &ReplayConfigSnapshot) -> Self {
        Self {
            mode: snapshot
                .get_str("link_play_mode")
                .unwrap_or("disabled")
                .to_string(),
            address: snapshot
                .get_str("link_play_address")
                .unwrap_or("")
                .to_string(),
            port: snapshot
                .get_value_string("link_play_port")
                .and_then(|value| value.parse::<u16>().ok())
                .filter(|port| *port > 0)
                .unwrap_or(55_435),
            lan_discovery: snapshot
                .get_value_string("link_play_lan_discovery")
                .is_none_or(|value| !value.eq_ignore_ascii_case("false")),
        }
    }

    pub fn validation_error(&self) -> Option<SettingsOutcome> {
        let mode = self.mode.trim();
        let address = self.address.trim();
        if !matches!(mode, "disabled" | "host" | "client") {
            Some(SettingsOutcome::InvalidMode)
        } else if (mode == "client" && address.is_empty())
            || address.len() > 255
            || address.chars().any(char::is_whitespace)
            || address.chars().any(char::is_control)
        {
            Some(SettingsOutcome::InvalidAddress)
        } else if self.port == 0 {
            Some(SettingsOutcome::InvalidPort)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_steps_default_after_100_percent() {
        for inherited in [None, Some(0), Some(5), Some(10)] {
            let mut values = vec![Some(0)];
            while let Some(next) =
                ReplaySetting::Volume.step_override(*values.last().unwrap(), inherited, true)
            {
                values.push(next);
            }
            assert_eq!(values, (0..=10).map(Some).chain([None]).collect::<Vec<_>>());
            for pair in values.windows(2) {
                assert_eq!(
                    ReplaySetting::Volume.step_override(pair[1], inherited, false),
                    Some(pair[0])
                );
            }
            assert_eq!(
                ReplaySetting::Volume.step_override(None, inherited, true),
                None
            );
        }
        assert_eq!(
            ReplaySetting::MonitorX.step_override(None, Some(4), true),
            Some(Some(6))
        );
    }
    #[test]
    fn active_identity_matches_library_subdirectories_not_just_filename() {
        let identity = ActiveGameIdentity {
            system: "arcade_fbneo".into(),
            game_file: "/media/nfs/roms/arcade_fbneo/Horizontal/00 Clean Romset/punisher.zip"
                .into(),
        };
        assert!(identity.matches_library_game(
            "arcade_fbneo",
            "/roms/arcade_fbneo/Horizontal/00 Clean Romset/punisher.zip"
        ));
        assert!(!identity.matches_library_game("arcade_fbneo", "/roms/arcade_fbneo/punisher.zip"));
        assert!(
            !identity.matches_library_game("arcade_fbneo", "/roms/arcade_fbneo/Other/punisher.zip")
        );
        assert!(!identity.matches_library_game(
            "arcade_mame",
            "/roms/arcade_fbneo/Horizontal/00 Clean Romset/punisher.zip"
        ));
    }

    #[test]
    fn link_play_validation_is_shared_and_accepts_ipv6() {
        let mut config = LinkPlayConfig {
            mode: "client".into(),
            address: "::1".into(),
            port: 55435,
            lan_discovery: false,
        };
        assert_eq!(config.validation_error(), None);
        config.address.clear();
        assert_eq!(
            config.validation_error(),
            Some(SettingsOutcome::InvalidAddress)
        );
        config.mode = "host".into();
        assert_eq!(config.validation_error(), None);
        config.port = 0;
        assert_eq!(
            config.validation_error(),
            Some(SettingsOutcome::InvalidPort)
        );
    }
    #[test]
    fn link_play_snapshot_uses_current_api_values_and_beta_defaults() {
        let defaults = LinkPlayConfig::from_snapshot(&ReplayConfigSnapshot::default());
        assert_eq!(defaults.mode, "disabled");
        assert_eq!(defaults.address, "");
        assert_eq!(defaults.port, 55_435);
        assert!(defaults.lan_discovery);

        let snapshot: ReplayConfigSnapshot = serde_json::from_str(
            r#"{"config":{"link_play_mode":"client","link_play_address":"::1","link_play_port":"6000","link_play_lan_discovery":false}}"#,
        )
        .unwrap();
        let configured = LinkPlayConfig::from_snapshot(&snapshot);
        assert_eq!(configured.mode, "client");
        assert_eq!(configured.address, "::1");
        assert_eq!(configured.port, 6000);
        assert!(!configured.lan_discovery);
    }
    #[test]
    fn inheritance_resolves_core_before_global_and_requires_known_values() {
        let game = ReplayConfigSnapshot::default();
        let core: ReplayConfigSnapshot = serde_json::from_str(
            r#"{"config":{"replay_video_monitor_x":4,"replay_video_monitor_y":"default"}}"#,
        )
        .unwrap();
        let global: ReplayConfigSnapshot =
            serde_json::from_str(r#"{"config":{"video_monitor_x":0,"video_monitor_y":2}}"#)
                .unwrap();
        let identity = ActiveGameIdentity {
            system: "snes".into(),
            game_file: "/roms/snes/game.sfc".into(),
        };
        let config = ActiveOverrideConfig::from_snapshots(
            identity.clone(),
            OverrideScope::Game,
            &game,
            &core,
            &global,
        );
        assert!(!config.configured);
        let x = config
            .overrides
            .iter()
            .find(|entry| entry.setting == ReplaySetting::MonitorX)
            .unwrap();
        assert_eq!((x.value, x.inherited), (None, Some(4)));
        let y = config
            .overrides
            .iter()
            .find(|entry| entry.setting == ReplaySetting::MonitorY)
            .unwrap();
        assert_eq!(y.inherited, Some(2));
        assert_eq!(config.overrides[0].inherited, None);
        let system = ActiveOverrideConfig::from_snapshots(
            identity,
            OverrideScope::System,
            &game,
            &core,
            &global,
        );
        let x = system
            .overrides
            .iter()
            .find(|entry| entry.setting == ReplaySetting::MonitorX)
            .unwrap();
        assert_eq!((x.value, x.inherited), (Some(4), Some(0)));
        let mut configured_game = game.clone();
        configured_game.configured = true;
        let configured = ActiveOverrideConfig::from_snapshots(
            system.identity,
            OverrideScope::Game,
            &configured_game,
            &core,
            &global,
        );
        let x = configured
            .overrides
            .iter()
            .find(|entry| entry.setting == ReplaySetting::MonitorX)
            .unwrap();
        assert_eq!((x.value, x.inherited), (None, Some(0)));
        assert!(configured.configured);
        let mut update = ActiveOverrideUpdate {
            identity: config.identity,
            scope: OverrideScope::Game,
            changes: vec![OverrideChange {
                setting: ReplaySetting::MonitorY,
                value: Some(4),
            }],
        };
        assert!(update.is_valid());
        update.changes.push(update.changes[0].clone());
        assert!(!update.is_valid());
        update.changes.pop();
        update.changes[0].value = Some(3);
        assert!(!update.is_valid());
        update.changes.clear();
        assert!(!update.is_valid());
    }
    #[test]
    fn settings_round_trip_and_reject_out_of_range_values() {
        for setting in ReplaySetting::ALL {
            let (min, max, step) = setting.bounds();
            for value in (min..=max).step_by(step as usize) {
                assert_eq!(setting.parse(&setting.encode(value)), Some(value));
            }
            assert_eq!(setting.parse(&setting.encode(min - 1)), None);
            assert_eq!(setting.parse(&setting.encode(max + 1)), None);
        }
        assert_eq!(ReplaySetting::Gamma.parse("NaN"), None);
        assert_eq!(ReplaySetting::MonitorX.parse("1"), None);
    }
    #[test]
    fn positions_step_from_effective_value_without_entering_inheritance() {
        assert_eq!(ReplaySetting::MonitorX.adjust(0, true), 2);
        assert_eq!(ReplaySetting::MonitorX.adjust(0, false), -2);
        assert_eq!(ReplaySetting::MonitorX.adjust(-64, false), -64);
    }

    #[test]
    fn crt_hides_lcd_only_settings() {
        assert_eq!(
            VideoSettingsDisplay::from_video_mode(Some("1")),
            VideoSettingsDisplay::Crt
        );
        assert_eq!(
            VideoSettingsDisplay::from_video_mode(Some("5")),
            VideoSettingsDisplay::Lcd
        );
        assert_eq!(
            VideoSettingsDisplay::from_video_mode(Some("0")),
            VideoSettingsDisplay::Unknown
        );
        for setting in ReplaySetting::ALL {
            assert_eq!(
                setting.available_in_video_mode(Some("1")),
                !matches!(
                    setting,
                    ReplaySetting::IntegerScale | ReplaySetting::VideoFilter
                )
            );
            assert!(setting.available_in_video_mode(Some("0")));
            assert!(setting.available_in_video_mode(None));
        }
        let global: ReplayConfigSnapshot =
            serde_json::from_str(r#"{"config":{"video_mode":"1"}}"#).unwrap();
        let identity = ActiveGameIdentity {
            system: "snes".into(),
            game_file: "/roms/snes/game.sfc".into(),
        };
        let config = ActiveOverrideConfig::from_snapshots(
            identity,
            OverrideScope::Game,
            &ReplayConfigSnapshot::default(),
            &ReplayConfigSnapshot::default(),
            &global,
        );
        assert_eq!(config.overrides.len(), 5);
    }
    #[test]
    fn missing_game_is_not_an_identity() {
        assert_eq!(
            ActiveGameIdentity::from_status(&StatusResponse::default()),
            None
        );
    }
}
