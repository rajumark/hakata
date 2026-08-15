use gpui::{App, Global, Hsla, WindowAppearance, hsla, rgb, transparent_black};
use serde::{Deserialize, Serialize};

/// The user's theme choice: follow the OS, or force light/dark.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    fn resolves_to_dark(self, system_appearance: WindowAppearance) -> bool {
        match self {
            Self::System => matches!(
                system_appearance,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
            Self::Light => false,
            Self::Dark => true,
        }
    }

    fn native_override(self) -> Option<WindowAppearance> {
        match self {
            Self::System => None,
            Self::Light => Some(WindowAppearance::Light),
            Self::Dark => Some(WindowAppearance::Dark),
        }
    }
}

/// The active preference, published globally so every view resolves the same
/// palette while the window's native appearance is what it is.
#[derive(Clone, Copy)]
struct ActiveThemePreference(ThemePreference);

impl Global for ActiveThemePreference {}

/// The current preference, defaulting to following the system.
pub fn theme_preference(cx: &App) -> ThemePreference {
    cx.try_global::<ActiveThemePreference>()
        .map(|active| active.0)
        .unwrap_or_default()
}

/// Publish a new preference and push it onto the native window appearance.
/// The palette is resolved lazily per frame, so a subsequent `cx.notify()`
/// repaints the app in the new theme.
pub fn set_theme_preference(preference: ThemePreference, cx: &mut App) {
    cx.set_global(ActiveThemePreference(preference));
    cx.set_window_appearance(preference.native_override());
}

/// Neutral graphite surfaces in the spirit of the learning project (Waku):
/// color is reserved for meaning.
#[derive(Clone, Copy)]
pub struct Theme {
    pub is_dark: bool,
    pub sidebar: Hsla,
    pub sidebar_drag_background: Hsla,
    pub sidebar_item_background: Hsla,
    pub surface: Hsla,
    pub raised: Hsla,
    pub overlay: Hsla,
    pub overlay_strong: Hsla,
    pub sidebar_border: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text_secondary: Hsla,
    pub text_tertiary: Hsla,
    pub text_ghost: Hsla,
    pub accent: Hsla,
    pub resize_handle: Hsla,
    pub danger: Hsla,
    pub success: Hsla,
}

impl Theme {
    pub fn current(cx: &App) -> Self {
        let preference = theme_preference(cx);
        if preference.resolves_to_dark(cx.window_appearance()) {
            Self::dark()
        } else {
            Self::light()
        }
    }

    pub fn dark() -> Self {
        Self {
            is_dark: true,
            sidebar: if cfg!(target_os = "macos") {
                transparent_black()
            } else {
                rgb(0x181818).into()
            },
            sidebar_drag_background: rgb(0x181818).into(),
            sidebar_item_background: hsla(0.0, 0.0, 0.941, 0.06),
            surface: rgb(0x1A1A1A).into(),
            raised: rgb(0x232323).into(),
            overlay: hsla(220.0 / 360.0, 0.10, 0.90, 0.05),
            overlay_strong: hsla(220.0 / 360.0, 0.10, 0.90, 0.09),
            sidebar_border: hsla(126.93 / 360.0, 0.000_000_1, 0.16077, 1.0),
            border: hsla(220.0 / 360.0, 0.10, 0.90, 0.07),
            border_strong: hsla(220.0 / 360.0, 0.10, 0.90, 0.14),
            text: rgb(0xE2E2E2).into(),
            text_secondary: rgb(0xA3A3A3).into(),
            text_tertiary: rgb(0x7D7D7D).into(),
            text_ghost: rgb(0x575757).into(),
            accent: rgb(0xE2795B).into(),
            resize_handle: rgb(0x3B82F6).into(),
            danger: rgb(0xE2726A).into(),
            success: rgb(0x62C987).into(),
        }
    }

    pub fn light() -> Self {
        Self {
            is_dark: false,
            sidebar: if cfg!(target_os = "macos") {
                transparent_black()
            } else {
                rgb(0xF3F3F3).into()
            },
            sidebar_drag_background: rgb(0xF3F3F3).into(),
            sidebar_item_background: hsla(0.0, 0.0, 0.078, 0.06),
            surface: rgb(0xF6F5F6).into(),
            raised: rgb(0xECECEC).into(),
            overlay: hsla(220.0 / 360.0, 0.10, 0.12, 0.05),
            overlay_strong: hsla(220.0 / 360.0, 0.10, 0.12, 0.09),
            sidebar_border: hsla(0.0, 0.0, 0.078, 0.12),
            border: hsla(220.0 / 360.0, 0.10, 0.12, 0.08),
            border_strong: hsla(220.0 / 360.0, 0.10, 0.12, 0.15),
            text: rgb(0x242424).into(),
            text_secondary: rgb(0x666666).into(),
            text_tertiary: rgb(0x858585).into(),
            text_ghost: rgb(0xA4A4A4).into(),
            accent: rgb(0xC85F44).into(),
            resize_handle: rgb(0x2563EB).into(),
            danger: rgb(0xC64A42).into(),
            success: rgb(0x2F8F52).into(),
        }
    }
}
