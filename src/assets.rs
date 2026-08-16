use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};

pub struct Assets;

macro_rules! icons {
    ($($name:literal),+ $(,)?) => {
        &[$((
            concat!("icons/", $name, ".svg"),
            include_bytes!(concat!("../assets/icons/", $name, ".svg")).as_slice(),
        )),+]
    };
}

const ICONS: &[(&str, &[u8])] = icons![
    "apps",
    "arrow-left",
    "arrow-right",
    "check",
    "chevron-down",
    "compose",
    "copy",
    "loader-circle",
    "panel-left",
    "refresh-cw",
    "search",
    "settings",
    "smartphone",
    "terminal-square",
    "x",
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
