use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::adb::AppFilter;
use crate::i18n::AppLanguage;
use crate::theme::ThemePreference;

/// Persisted user settings, stored as JSON in the app-data dir.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemePreference,
    pub language: AppLanguage,
    pub pinned_apps: Vec<String>,
    pub apps_filter: AppFilter,
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
    fn round_trips_theme_and_language_preferences() {
        for preference in ThemePreference::ALL {
            let settings = Settings {
                theme: preference,
                language: AppLanguage::English,
                pinned_apps: vec!["com.example.one".into(), "com.example.two".into()],
                apps_filter: AppFilter::System,
            };
            let json = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(restored.theme, preference);
            assert_eq!(restored.language, AppLanguage::English);
            assert_eq!(restored.pinned_apps.len(), 2);
            assert_eq!(restored.apps_filter, AppFilter::System);
        }
    }

    #[test]
    fn defaults_when_corrupt() {
        let restored: Settings = serde_json::from_str("not json").unwrap_or_default();
        assert_eq!(restored.theme, ThemePreference::System);
        assert_eq!(restored.language, AppLanguage::System);
        assert!(restored.pinned_apps.is_empty());
        assert_eq!(restored.apps_filter, AppFilter::User);
    }

    #[test]
    fn missing_fields_default() {
        let restored: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(restored.theme, ThemePreference::Dark);
        assert_eq!(restored.language, AppLanguage::System);
        assert!(restored.pinned_apps.is_empty());
        assert_eq!(restored.apps_filter, AppFilter::User);
    }

    #[test]
    fn language_serializes_as_kebab_case() {
        let settings = Settings {
            language: AppLanguage::SimplifiedChinese,
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains(r#""language":"simplified-chinese""#));
    }
}
