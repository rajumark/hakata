use serde::{Deserialize, Serialize};

/// The language preference the user persists. `System` resolves to one of the
/// shipped locales at runtime.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppLanguage {
    #[default]
    System,
    English,
    SimplifiedChinese,
    Japanese,
}

impl AppLanguage {
    pub const ALL: [Self; 4] = [
        Self::System,
        Self::English,
        Self::SimplifiedChinese,
        Self::Japanese,
    ];

    pub fn locale(self) -> &'static str {
        match self.resolved() {
            Self::System => unreachable!("system language always resolves to a shipped locale"),
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
            Self::Japanese => "ja",
        }
    }

    /// Explicit language names are autonyms so the selector stays usable even
    /// when the current locale is unfamiliar.
    pub fn label(self) -> String {
        match self {
            Self::System => translate("language.system"),
            Self::English => "English".to_owned(),
            Self::SimplifiedChinese => "简体中文".to_owned(),
            Self::Japanese => "日本語".to_owned(),
        }
    }

    pub fn resolved(self) -> Self {
        match self {
            Self::System => Self::from_system(),
            explicit => explicit,
        }
    }

    fn from_system() -> Self {
        Self::from_locale_id(&system_locale())
    }

    fn from_locale_id(locale: &str) -> Self {
        let locale = locale.replace('_', "-").to_ascii_lowercase();
        if locale == "zh-cn" || locale == "zh-sg" || locale.starts_with("zh-hans") {
            Self::SimplifiedChinese
        } else if locale == "ja" || locale.starts_with("ja-") {
            Self::Japanese
        } else {
            Self::English
        }
    }
}

pub fn set_language(language: AppLanguage) {
    rust_i18n::set_locale(language.locale());
}

pub fn translate(key: &str) -> String {
    rust_i18n::t!(key).into_owned()
}

fn system_locale() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "en".to_owned())
        .split('.')
        .next()
        .unwrap_or("en")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_locale_ids_are_supported() {
        assert_eq!(AppLanguage::English.locale(), "en");
        assert_eq!(AppLanguage::SimplifiedChinese.locale(), "zh-CN");
        assert_eq!(AppLanguage::Japanese.locale(), "ja");
        let locales = rust_i18n::available_locales!();
        assert_eq!(locales.len(), 3);
        assert!(locales.iter().any(|locale| locale.as_ref() == "en"));
        assert!(locales.iter().any(|locale| locale.as_ref() == "zh-CN"));
        assert!(locales.iter().any(|locale| locale.as_ref() == "ja"));
    }

    #[test]
    fn language_names_are_autonyms() {
        assert_eq!(AppLanguage::English.label(), "English");
        assert_eq!(AppLanguage::SimplifiedChinese.label(), "简体中文");
        assert_eq!(AppLanguage::Japanese.label(), "日本語");
    }

    #[test]
    fn system_is_the_default_persisted_preference_and_resolves_to_a_shipped_locale() {
        assert_eq!(AppLanguage::default(), AppLanguage::System);
        assert_eq!(
            serde_json::to_string(&AppLanguage::System).unwrap(),
            r#""system""#
        );
        assert!(matches!(
            AppLanguage::System.locale(),
            "en" | "zh-CN" | "ja"
        ));
    }

    #[test]
    fn japanese_system_locales_are_detected() {
        assert_eq!(AppLanguage::from_locale_id("ja"), AppLanguage::Japanese);
        assert_eq!(AppLanguage::from_locale_id("ja_JP"), AppLanguage::Japanese);
    }

    #[test]
    fn simplified_chinese_system_locales_are_detected_without_enabling_traditional_chinese() {
        assert_eq!(
            AppLanguage::from_locale_id("zh-Hans-CN"),
            AppLanguage::SimplifiedChinese
        );
        assert_eq!(
            AppLanguage::from_locale_id("zh_SG"),
            AppLanguage::SimplifiedChinese
        );
        assert_eq!(
            AppLanguage::from_locale_id("zh-Hant-TW"),
            AppLanguage::English
        );
    }

    #[test]
    fn translations_are_complete_and_interpolate_naturally() {
        assert_eq!(&*rust_i18n::t!("page.settings", locale = "zh-CN"), "设置");
        assert_eq!(&*rust_i18n::t!("page.settings", locale = "ja"), "設定");
        assert_eq!(&*rust_i18n::t!("theme.light", locale = "zh-CN"), "浅色");
        assert_eq!(&*rust_i18n::t!("theme.light", locale = "ja"), "ライト");
        assert_eq!(
            &*rust_i18n::t!("language.system", locale = "zh-CN"),
            "跟随系统"
        );
        assert_eq!(
            &*rust_i18n::t!("language.system", locale = "ja"),
            "システム"
        );
        assert_eq!(
            &*rust_i18n::t!(
                "permissions.opened_app_info",
                locale = "zh-CN",
                package = "com.example"
            ),
            "已打开 com.example 的应用信息"
        );
        assert_eq!(
            &*rust_i18n::t!(
                "permissions.opened_app_info",
                locale = "ja",
                package = "com.example"
            ),
            "com.example のアプリ情報を開きました"
        );
        assert_eq!(
            &*rust_i18n::t!(
                "action.granted_all",
                locale = "zh-CN",
                package = "com.example",
                total = 3
            ),
            "已为 com.example 授予全部 3 个权限"
        );
        assert_eq!(
            &*rust_i18n::t!(
                "action.granted_all",
                locale = "ja",
                package = "com.example",
                total = 3
            ),
            "com.example の権限 3 件をすべて許可しました"
        );
    }
}
