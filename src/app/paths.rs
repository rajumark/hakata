use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, ClipboardItem, Context, Div, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, SharedString, Stateful, Styled,
    div, px, prelude::*,
};

use crate::theme::Theme;

use super::{Hakata, app_actions::AppActionStatus, icon};

const MONO_FAMILY: &str = "JetBrains Mono";

/// One known filesystem path for an installed package, grouped by category.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PathItem {
    pub(crate) label: String,
    pub(crate) path: String,
    pub(crate) category: PathCategory,
}

/// The category groups used by the Paths tab, in display order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PathCategory {
    Installation,
    InternalData,
    ExternalStorage,
    Runtime,
    System,
}

impl PathCategory {
    pub(crate) fn label(self) -> String {
        match self {
            Self::Installation => tr!("paths.category.installation"),
            Self::InternalData => tr!("paths.category.internal_data"),
            Self::ExternalStorage => tr!("paths.category.external_storage"),
            Self::Runtime => tr!("paths.category.runtime"),
            Self::System => tr!("paths.category.system"),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Installation => "icons/archive.svg",
            Self::InternalData => "icons/folder.svg",
            Self::ExternalStorage => "icons/hard-drive.svg",
            Self::Runtime => "icons/gauge.svg",
            Self::System => "icons/settings.svg",
        }
    }
}

/// Cache state for the `pm path <pkg>` probe, kept in sync with the package
/// dump so the Paths tab renders from cached state only.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PathsState {
    pub(crate) device: Option<SharedString>,
    pub(crate) package: Option<SharedString>,
    pub(crate) raw: Option<SharedString>,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) epoch: usize,
    pub(crate) collapsed: Vec<PathCategory>,
}

/// The `dumpsys package` fields the path builder reads.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct DumpFields {
    data_dir: Option<String>,
    code_path: Option<String>,
    resource_path: Option<String>,
    native_lib_dir: Option<String>,
    primary_cpu_abi: Option<String>,
    user_id: Option<String>,
}

const DATA_SUBPATHS: [(&str, &str); 5] = [
    ("Shared Preferences", "shared_prefs"),
    ("Databases", "databases"),
    ("Cache", "cache"),
    ("Code Cache", "code_cache"),
    ("Files", "files"),
];

/// APK paths from `pm path <pkg>` output (`package:/path` lines).
fn parse_pm_paths(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("package:").map(str::trim))
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

/// The install folder an APK lives in: everything before the last `/` of a
/// path that names `base.apk` (or ends in `.apk`).
fn extract_base_path(apk_path: &str) -> Option<String> {
    if !apk_path.contains("/base.apk") && !apk_path.ends_with(".apk") {
        return None;
    }
    let last_slash = apk_path.rfind('/')?;
    if last_slash == 0 {
        return None;
    }
    Some(apk_path[..last_slash].to_string())
}

/// The next whitespace-delimited token after `key` anywhere in `line`.
fn value_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.find(key).map(|start| {
        line[start + key.len()..]
            .split_whitespace()
            .next()
            .unwrap_or("")
    })
}

fn parse_dump_fields(dump: &str) -> DumpFields {
    let mut fields = DumpFields::default();
    for line in dump.lines() {
        let line = line.trim();
        if fields.data_dir.is_none()
            && let Some(value) = value_after(line, "dataDir=")
        {
            fields.data_dir = Some(value.to_string());
        }
        if fields.code_path.is_none()
            && let Some(value) = value_after(line, "codePath=")
        {
            fields.code_path = Some(value.to_string());
        }
        if fields.resource_path.is_none()
            && let Some(value) = value_after(line, "resourcePath=")
        {
            fields.resource_path = Some(value.to_string());
        }
        if fields.native_lib_dir.is_none()
            && let Some(value) = value_after(line, "legacyNativeLibraryDir=")
        {
            fields.native_lib_dir = Some(value.to_string());
        }
        if fields.primary_cpu_abi.is_none()
            && let Some(value) = value_after(line, "primaryCpuAbi=")
        {
            fields.primary_cpu_abi = Some(value.to_string());
        }
        if fields.user_id.is_none()
            && let Some(value) = value_after(line, "userId=")
            && !value.is_empty()
            && value.chars().all(|ch| ch.is_ascii_digit())
        {
            fields.user_id = Some(value.to_string());
        }
    }
    fields
}

/// Assemble the full path list for a package, mirroring the Porpita service:
/// APK locations from `pm path`, then the derived paths from the `dumpsys`
/// fields. Categories follow the order of first appearance.
fn build_path_items(pm_path_output: &str, dump: &str, package: &str) -> Vec<PathItem> {
    let apk_paths = parse_pm_paths(pm_path_output);
    let fields = parse_dump_fields(dump);
    let mut items: Vec<PathItem> = Vec::new();

    let mut base_path: Option<String> = None;
    if let Some(first_apk) = apk_paths.first() {
        base_path = extract_base_path(first_apk);
        items.push(PathItem {
            label: "APK".to_string(),
            path: first_apk.clone(),
            category: PathCategory::Installation,
        });
        for (index, apk) in apk_paths.iter().enumerate().skip(1) {
            items.push(PathItem {
                label: format!("Split APK {}", index),
                path: apk.clone(),
                category: PathCategory::Installation,
            });
        }
        if let Some(base) = &base_path {
            items.push(PathItem {
                label: "Code Path".to_string(),
                path: base.clone(),
                category: PathCategory::Installation,
            });
            items.push(PathItem {
                label: "Native Libraries".to_string(),
                path: format!("{}/lib", base),
                category: PathCategory::Installation,
            });
            items.push(PathItem {
                label: "OAT Directory".to_string(),
                path: format!("{}/oat", base),
                category: PathCategory::Runtime,
            });
        }
    }

    if let Some(code_path) = fields.code_path.as_ref()
        && !code_path.is_empty()
        && !items.iter().any(|item| item.label == "Code Path")
    {
        items.push(PathItem {
            label: "Code Path".to_string(),
            path: code_path.clone(),
            category: PathCategory::Installation,
        });
    }

    if let Some(resource_path) = fields.resource_path.as_ref()
        && !resource_path.is_empty()
        && Some(resource_path.as_str()) != fields.code_path.as_deref()
    {
        items.push(PathItem {
            label: "Resource Path".to_string(),
            path: resource_path.clone(),
            category: PathCategory::Installation,
        });
    }

    if let Some(native_lib_dir) = fields.native_lib_dir.as_ref()
        && !native_lib_dir.is_empty()
        && !items.iter().any(|item| item.path == *native_lib_dir)
    {
        items.push(PathItem {
            label: "Native Library Dir".to_string(),
            path: native_lib_dir.clone(),
            category: PathCategory::Installation,
        });
    }

    let data_root = fields
        .data_dir
        .as_ref()
        .filter(|dir| !dir.is_empty())
        .cloned()
        .or_else(|| {
            fields
                .user_id
                .as_ref()
                .map(|user_id| format!("/data/user/{}/{}", user_id, package))
        });
    if let Some(root) = &data_root {
        items.push(PathItem {
            label: "Data Directory".to_string(),
            path: root.clone(),
            category: PathCategory::InternalData,
        });
        for (label, sub) in DATA_SUBPATHS {
            items.push(PathItem {
                label: label.to_string(),
                path: format!("{}/{}", root, sub),
                category: PathCategory::InternalData,
            });
        }
    }

    items.push(PathItem {
        label: "Data (symlink)".to_string(),
        path: format!("/data/data/{}", package),
        category: PathCategory::InternalData,
    });

    items.extend([
        PathItem {
            label: "External Data".to_string(),
            path: format!("/storage/emulated/0/Android/data/{}", package),
            category: PathCategory::ExternalStorage,
        },
        PathItem {
            label: "External Files".to_string(),
            path: format!("/storage/emulated/0/Android/data/{}/files", package),
            category: PathCategory::ExternalStorage,
        },
        PathItem {
            label: "External Cache".to_string(),
            path: format!("/storage/emulated/0/Android/data/{}/cache", package),
            category: PathCategory::ExternalStorage,
        },
        PathItem {
            label: "External Media".to_string(),
            path: format!("/storage/emulated/0/Android/media/{}", package),
            category: PathCategory::ExternalStorage,
        },
        PathItem {
            label: "OBB Files".to_string(),
            path: format!("/storage/emulated/0/Android/obb/{}", package),
            category: PathCategory::ExternalStorage,
        },
    ]);

    if let Some(user_id) = &fields.user_id {
        items.extend([
            PathItem {
                label: "Profile (Current)".to_string(),
                path: format!("/data/misc/profiles/cur/{}/{}", user_id, package),
                category: PathCategory::Runtime,
            },
            PathItem {
                label: "Profile (Reference)".to_string(),
                path: format!("/data/misc/profiles/ref/{}", package),
                category: PathCategory::Runtime,
            },
        ]);
    }

    if let Some(abi) = fields.primary_cpu_abi.as_ref()
        && !abi.is_empty()
        && abi != "null"
        && let Some(oat_base) = fields.code_path.as_deref().or(base_path.as_deref())
    {
        items.extend([
            PathItem {
                label: "Compiled DEX (ODEX)".to_string(),
                path: format!("{}/oat/{}/base.odex", oat_base, abi),
                category: PathCategory::Runtime,
            },
            PathItem {
                label: "Verified DEX (VDEX)".to_string(),
                path: format!("{}/oat/{}/base.vdex", oat_base, abi),
                category: PathCategory::Runtime,
            },
        ]);
    }

    items.extend([
        PathItem {
            label: "Package Registry".to_string(),
            path: "/data/system/packages.xml".to_string(),
            category: PathCategory::System,
        },
        PathItem {
            label: "Component Restrictions".to_string(),
            path: "/data/system/users/0/package-restrictions.xml".to_string(),
            category: PathCategory::System,
        },
    ]);

    items
}

/// Translate a row label produced by `build_path_items` at render time. The
/// builder keeps English labels so its tests stay deterministic; the mapping
/// mirrors the `paths.*` catalog keys.
fn translate_path_label(label: &str) -> String {
    match label {
        "APK" => tr!("paths.apk"),
        "Code Path" => tr!("paths.code_path"),
        "Native Libraries" => tr!("paths.native_libraries"),
        "OAT Directory" => tr!("paths.oat_directory"),
        "Resource Path" => tr!("paths.resource_path"),
        "Native Library Dir" => tr!("paths.native_library_dir"),
        "Data Directory" => tr!("paths.data_directory"),
        "Shared Preferences" => tr!("paths.shared_preferences"),
        "Databases" => tr!("paths.databases"),
        "Cache" => tr!("paths.cache"),
        "Code Cache" => tr!("paths.code_cache"),
        "Files" => tr!("paths.files"),
        "Data (symlink)" => tr!("paths.data_symlink"),
        "External Data" => tr!("paths.external_data"),
        "External Files" => tr!("paths.external_files"),
        "External Cache" => tr!("paths.external_cache"),
        "External Media" => tr!("paths.external_media"),
        "OBB Files" => tr!("paths.obb"),
        "Profile (Current)" => tr!("paths.profile_current"),
        "Profile (Reference)" => tr!("paths.profile_reference"),
        "Compiled DEX (ODEX)" => tr!("paths.odex"),
        "Verified DEX (VDEX)" => tr!("paths.vdex"),
        "Package Registry" => tr!("paths.package_registry"),
        "Component Restrictions" => tr!("paths.component_restrictions"),
        _ => label
            .strip_prefix("Split APK ")
            .map_or_else(|| label.to_string(), |index| tr!("paths.split_apk", index = index)),
    }
}

impl Hakata {
    fn reset_paths(&mut self) {
        self.paths.device = None;
        self.paths.package = None;
        self.paths.raw = None;
        self.paths.loading = false;
        self.paths.error = None;
        self.paths.epoch = 0;
    }

    /// Fetch `adb -s <device> shell pm path <pkg>` on the background executor
    /// and stash the output on the entity. Mirrors `fetch_package_dump` so the
    /// Paths tab never does I/O from render; a generation counter drops a
    /// superseded result.
    pub(crate) fn fetch_package_paths(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(serial) = self.selected_device.clone() else {
            self.reset_paths();
            cx.notify();
            return;
        };
        let Some(package) = self.selected_package.clone() else {
            self.reset_paths();
            cx.notify();
            return;
        };
        if !force
            && self.paths.device.as_deref() == Some(serial.as_str())
            && self.paths.package.as_deref() == Some(package.as_str())
            && self.paths.raw.is_some()
        {
            return;
        }
        if !crate::adb::is_installed() {
            self.reset_paths();
            self.paths.device = Some(serial);
            self.paths.package = Some(package);
            cx.notify();
            return;
        }
        self.paths.epoch += 1;
        let epoch = self.paths.epoch;
        let serial_for_spawn = serial.clone();
        let package_for_spawn = package.clone();
        self.paths.loading = true;
        self.paths.device = Some(serial);
        self.paths.package = Some(package);
        self.paths.error = None;
        cx.notify();
        let adb_path = crate::adb::adb_path();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    std::process::Command::new(&adb_path)
                        .arg("-s")
                        .arg(serial_for_spawn.as_str())
                        .arg("shell")
                        .arg("pm")
                        .arg("path")
                        .arg(package_for_spawn.as_str())
                        .output()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.paths.epoch != epoch {
                    return;
                }
                this.paths.loading = false;
                match result {
                    Ok(output) if output.status.success() => {
                        this.paths.raw =
                            Some(SharedString::from(String::from_utf8_lossy(&output.stdout)));
                        this.paths.error = None;
                    }
                    Ok(output) => {
                        this.paths.raw = None;
                        this.paths.error =
                            Some(String::from_utf8_lossy(&output.stderr).trim().to_string());
                    }
                    Err(error) => {
                        this.paths.raw = None;
                        this.paths.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn toggle_path_category(&mut self, category: PathCategory, cx: &mut Context<Self>) {
        if let Some(index) = self.paths.collapsed.iter().position(|c| *c == category) {
            self.paths.collapsed.remove(index);
        } else {
            self.paths.collapsed.push(category);
        }
        cx.notify();
    }

    /// The body of the Paths tab: the package's known paths grouped by
    /// category, assembled from the cached `pm path` and `dumpsys` output.
    pub(crate) fn render_paths_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let package = match &self.selected_package {
            Some(package) => package.clone(),
            None => return self.render_paths_center(&theme, &tr!("apps.no_app_selected")),
        };
        if self.package_dump_loading || self.paths.loading {
            return self.render_paths_loading(&theme);
        }
        if let Some(error) = &self.package_dump_error {
            return self.render_paths_error(&theme, error, cx);
        }
        if let Some(error) = &self.paths.error {
            return self.render_paths_error(&theme, error, cx);
        }
        let Some(dump) = self.package_dump_raw.as_deref() else {
            return self.render_paths_center(&theme, &tr!("paths.none"));
        };

        let items = build_path_items(self.paths.raw.as_deref().unwrap_or(""), dump, package.as_str());
        if items.is_empty() {
            return self.render_paths_center(&theme, &tr!("paths.none"));
        }

        let mut categories: Vec<PathCategory> = Vec::new();
        for item in &items {
            if !categories.contains(&item.category) {
                categories.push(item.category);
            }
        }

        let mut column = div().flex().flex_col().gap(px(2.0));
        let mut row_index = 0usize;
        for category in &categories {
            let members: Vec<&PathItem> = items
                .iter()
                .filter(|item| item.category == *category)
                .collect();
            let start = row_index;
            row_index += members.len();
            column = column.child(self.render_path_category(&theme, *category, &members, start, cx));
        }

        div()
            .id("apps-paths")
            .size_full()
            .overflow_y_scroll()
            .py(px(8.0))
            .child(column)
            .into_any_element()
    }

    fn render_path_category(
        &self,
        theme: &Theme,
        category: PathCategory,
        members: &[&PathItem],
        start_index: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let header = self.render_path_category_header(theme, category, members.len(), cx);
        let mut section = div().flex().flex_col().child(header);
        if !self.paths.collapsed.contains(&category) {
            for (offset, item) in members.iter().enumerate() {
                section = section.child(self.render_path_row(theme, item, start_index + offset, cx));
            }
        }
        section
    }

    fn render_path_category_header(
        &self,
        theme: &Theme,
        category: PathCategory,
        count: usize,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let collapsed = self.paths.collapsed.contains(&category);
        div()
            .id(SharedString::from(format!(
                "paths-category-{}",
                category.label().to_lowercase().replace(' ', "-")
            )))
            .tab_index(0)
            .cursor_default()
            .h(px(28.0))
            .flex_none()
            .px(px(8.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .text_size(px(12.0))
            .text_color(theme.text)
            .focus_visible(|style| style.bg(theme.overlay))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_path_category(category, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.toggle_path_category(category, cx);
                    cx.stop_propagation();
                }
            }))
            .child(icon(category.icon(), 14.0, theme.accent))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(category.label()),
            )
            .child(
                div()
                    .h(px(16.0))
                    .px(px(6.0))
                    .rounded(px(8.0))
                    .bg(theme.overlay_strong)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.0))
                    .text_color(theme.text_tertiary)
                    .child(SharedString::from(format!("{}", count))),
            )
            .child(
                icon("icons/chevron-right.svg", 12.0, theme.text_tertiary).when(
                    !collapsed,
                    |element| {
                        element.with_transformation(gpui::Transformation::rotate(gpui::percentage(
                            0.25,
                        )))
                    },
                ),
            )
    }

    fn render_path_row(
        &self,
        theme: &Theme,
        item: &PathItem,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let group = SharedString::from(format!("paths-row-{index}"));
        let path = item.path.clone();
        div()
            .id(SharedString::from(format!("paths-path-{index}")))
            .group(group.clone())
            .py(px(3.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .flex()
            .items_start()
            .gap(px(8.0))
            .hover(|element| element.bg(theme.overlay))
            .child(
                div()
                    .w(px(140.0))
                    .flex_none()
                    .truncate()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .child(translate_path_label(&item.label)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(MONO_FAMILY)
                    .text_size(px(11.0))
                    .text_color(theme.text)
                    .child(SharedString::from(path.clone())),
            )
            .child(
                div()
                    .id(SharedString::from(format!("paths-copy-{index}")))
                    .tab_index(0)
                    .size(px(20.0))
                    .flex_none()
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .opacity(0.0)
                    .group_hover(group, |element| element.opacity(1.0))
                    .focus_visible(|style| style.bg(theme.overlay).opacity(1.0))
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .child(icon("icons/copy.svg", 11.0, theme.text_tertiary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.copy_path(path.clone(), cx);
                    })),
            )
    }

    fn copy_path(&mut self, path: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
        self.app_action_status = Some(AppActionStatus::Done {
            message: tr!("paths.copied", path = path),
        });
        cx.notify();
    }

    fn render_paths_center(&self, theme: &Theme, message: &str) -> AnyElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .px(px(16.0))
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.text_ghost)
                    .child(SharedString::from(message)),
            )
            .into_any_element()
    }

    fn render_paths_loading(&self, theme: &Theme) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .child(
                icon("icons/loader-circle.svg", 14.0, theme.text_tertiary).with_animation(
                    SharedString::from("apps-paths-loading-spinner"),
                    Animation::new(Duration::from_millis(900))
                        .repeat()
                        .with_easing(gpui::linear),
                    |icon, delta| {
                        icon.with_transformation(gpui::Transformation::rotate(gpui::percentage(
                            delta,
                        )))
                    },
                ),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.text_tertiary)
                    .child(tr_cow!("paths.loading")),
            )
            .into_any_element()
    }

    fn render_paths_error(&self, theme: &Theme, message: &str, cx: &mut Context<Self>) -> AnyElement {
        let retry = div()
            .id("apps-paths-retry")
            .tab_index(0)
            .h(px(24.0))
            .px(px(10.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(theme.border_strong)
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .text_size(px(11.0))
            .text_color(theme.text_secondary)
            .hover(|element| element.bg(theme.overlay))
            .focus_visible(|style| style.border_1().border_color(theme.accent))
            .on_click(cx.listener(|this, _, _, cx| {
                this.fetch_package_dump(true, cx);
                this.fetch_package_paths(true, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.fetch_package_dump(true, cx);
                    this.fetch_package_paths(true, cx);
                    cx.stop_propagation();
                }
            }))
            .child(tr_cow!("common.retry"));
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .px(px(16.0))
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.danger)
                    .child(SharedString::from(message)),
            )
            .child(retry)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PM_PATH: &str = "\
package:/data/app/~~abc==/com.example.app/base.apk
package:/data/app/~~abc==/com.example.app/split_config.en.apk
";

    const DUMP: &str = "\
Package [com.example.app] (abcdef):
  userId=10123
  dataDir=/data/user/0/com.example.app
  codePath=/data/app/~~abc==/com.example.app/base.apk
  resourcePath=/data/app/~~abc==/com.example.app/base.apk
  legacyNativeLibraryDir=/data/app/~~abc==/com.example.app/lib/arm64
  primaryCpuAbi=arm64-v8a
";

    fn labels(items: &[PathItem]) -> Vec<&str> {
        items.iter().map(|item| item.label.as_str()).collect()
    }

    #[test]
    fn parses_pm_path_lines() {
        assert_eq!(
            parse_pm_paths(PM_PATH),
            vec![
                "/data/app/~~abc==/com.example.app/base.apk",
                "/data/app/~~abc==/com.example.app/split_config.en.apk",
            ]
        );
        assert!(parse_pm_paths("").is_empty());
        assert!(parse_pm_paths("no such file\n").is_empty());
    }

    #[test]
    fn extracts_base_folder_only_from_apk_paths() {
        assert_eq!(
            extract_base_path("/data/app/~~abc==/com.example.app/base.apk").as_deref(),
            Some("/data/app/~~abc==/com.example.app")
        );
        assert_eq!(
            extract_base_path("/data/app/~~abc==/com.example.app/split.apk").as_deref(),
            Some("/data/app/~~abc==/com.example.app")
        );
        assert_eq!(extract_base_path("/data/app/x/base.odex"), None);
        assert_eq!(extract_base_path("no-slash.apk"), None);
    }

    #[test]
    fn parses_dump_fields() {
        let fields = parse_dump_fields(DUMP);
        assert_eq!(fields.data_dir.as_deref(), Some("/data/user/0/com.example.app"));
        assert_eq!(
            fields.code_path.as_deref(),
            Some("/data/app/~~abc==/com.example.app/base.apk")
        );
        assert_eq!(
            fields.resource_path.as_deref(),
            Some("/data/app/~~abc==/com.example.app/base.apk")
        );
        assert_eq!(
            fields.native_lib_dir.as_deref(),
            Some("/data/app/~~abc==/com.example.app/lib/arm64")
        );
        assert_eq!(fields.primary_cpu_abi.as_deref(), Some("arm64-v8a"));
        assert_eq!(fields.user_id.as_deref(), Some("10123"));
    }

    #[test]
    fn ignores_non_numeric_user_ids() {
        assert_eq!(parse_dump_fields("  userId=null\n").user_id, None);
        assert_eq!(parse_dump_fields("  userId=\n").user_id, None);
    }

    #[test]
    fn assembles_all_categories_in_reference_order() {
        let items = build_path_items(PM_PATH, DUMP, "com.example.app");
        assert_eq!(
            labels(&items),
            vec![
                "APK",
                "Split APK 1",
                "Code Path",
                "Native Libraries",
                "OAT Directory",
                "Native Library Dir",
                "Data Directory",
                "Shared Preferences",
                "Databases",
                "Cache",
                "Code Cache",
                "Files",
                "Data (symlink)",
                "External Data",
                "External Files",
                "External Cache",
                "External Media",
                "OBB Files",
                "Profile (Current)",
                "Profile (Reference)",
                "Compiled DEX (ODEX)",
                "Verified DEX (VDEX)",
                "Package Registry",
                "Component Restrictions",
            ]
        );

        let mut categories: Vec<PathCategory> = Vec::new();
        for item in &items {
            if !categories.contains(&item.category) {
                categories.push(item.category);
            }
        }
        assert_eq!(
            categories,
            vec![
                PathCategory::Installation,
                PathCategory::Runtime,
                PathCategory::InternalData,
                PathCategory::ExternalStorage,
                PathCategory::System,
            ]
        );
    }

    #[test]
    fn dedups_shared_paths_and_skips_equal_resource_path() {
        let items = build_path_items(PM_PATH, DUMP, "com.example.app");
        let apks = items
            .iter()
            .filter(|item| item.label == "APK" || item.label == "Split APK 1");
        assert_eq!(apks.count(), 2);
        assert!(!items.iter().any(|item| item.label == "Resource Path"));
        let native = items.iter().find(|item| item.label == "Native Library Dir");
        assert_eq!(
            native.map(|item| item.path.as_str()),
            Some("/data/app/~~abc==/com.example.app/lib/arm64")
        );
    }

    #[test]
    fn adds_resource_path_when_it_differs_from_code_path() {
        let dump = "codePath=/data/app/x.apk\nresourcePath=/data/app/y.apk\n";
        let items = build_path_items("", dump, "com.example.app");
        assert!(items.iter().any(|item| {
            item.label == "Resource Path" && item.path == "/data/app/y.apk"
        }));
    }

    #[test]
    fn adds_dump_code_path_when_no_pm_output() {
        let dump = "codePath=/data/app/x.apk\n";
        let items = build_path_items("", dump, "com.example.app");
        assert!(items
            .iter()
            .any(|item| item.label == "Code Path" && item.path == "/data/app/x.apk"));
    }

    #[test]
    fn falls_back_to_user_data_dir_when_data_dir_missing() {
        let dump = "Package [x] (1):\n  userId=10123\n";
        let items = build_path_items("", dump, "com.example.app");
        assert!(items
            .iter()
            .any(|item| item.path == "/data/user/10123/com.example.app"));
        assert!(items.iter().any(|item| item.path == "/data/data/com.example.app"));
    }

    #[test]
    fn skips_odex_without_abi() {
        let dump = "dataDir=/data/user/0/com.example.app\n";
        let items = build_path_items("", dump, "com.example.app");
        assert!(!items.iter().any(|item| item.label == "Compiled DEX (ODEX)"));
        assert!(!items.iter().any(|item| item.label == "Verified DEX (VDEX)"));
    }

    #[test]
    fn always_lists_system_paths() {
        let items = build_path_items("", "no useful keys", "com.example.app");
        assert!(items
            .iter()
            .any(|item| item.path == "/data/system/packages.xml"));
        assert!(items.iter().any(|item| {
            item.path == "/data/system/users/0/package-restrictions.xml"
        }));
    }
}
