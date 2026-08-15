use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::theme::ThemePreference;

/// Persisted user settings, stored as JSON in the app-data dir.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemePreference,
}

fn settings_path() -> std::path::PathBuf {
    crate::adb::app_data_dir().join("settings.json")
}

/// Load settings from disk, defaulting on any read/parse failure so a corrupt
/// file can never prevent the app from starting.
pub fn load() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

/// Persist settings, best-effort.
pub fn save(settings: &Settings) -> Result<()> {
    let dir = crate::adb::app_data_dir();
    std::fs::create_dir_all(&dir)?;
    let data = serde_json::to_string_pretty(settings)?;
    std::fs::write(settings_path(), data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_theme_preference() {
        for preference in ThemePreference::ALL {
            let settings = Settings { theme: preference };
            let json = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(restored.theme, preference);
        }
    }

    #[test]
    fn defaults_when_corrupt() {
        let restored: Settings = serde_json::from_str("not json").unwrap_or_default();
        assert_eq!(restored.theme, ThemePreference::System);
    }
}
