use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, Bounds, Context, Div, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Pixels, SharedString, Stateful, Styled, Window, canvas, div, px,
    prelude::*,
};

use crate::theme::Theme;

use super::{Hakata, app_actions::AppActionStatus, icon};

/// The four permission views, mirroring the Porpita segmented switcher.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum PermissionsSegment {
    #[default]
    Runtime,
    Install,
    Declared,
    Requested,
}

impl PermissionsSegment {
    pub(crate) const ALL: [Self; 4] = [
        Self::Runtime,
        Self::Install,
        Self::Declared,
        Self::Requested,
    ];

    pub(crate) fn label(self) -> String {
        match self {
            Self::Runtime => tr!("permissions.segment.runtime"),
            Self::Install => tr!("permissions.segment.install"),
            Self::Declared => tr!("permissions.segment.declared"),
            Self::Requested => tr!("permissions.segment.requested"),
        }
    }
}

/// A permission in the `runtime permissions:` block, with its current grant
/// state as reported by the system (used by the runtime switch toggles).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RuntimePermission {
    pub(crate) name: SharedString,
    pub(crate) granted: bool,
}

/// A permission in the `install permissions:` block. `granted` is `None` when
/// the entry carries no explicit grant flag.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct InstallPermission {
    pub(crate) name: SharedString,
    pub(crate) granted: Option<bool>,
}

/// A permission declared by the app manifest, with its protection level.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DeclaredPermission {
    pub(crate) name: SharedString,
    pub(crate) protection: SharedString,
}

/// Cache state for the Permissions tab, parsed from the package dump.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PermissionsState {
    pub(crate) segment: PermissionsSegment,
    pub(crate) runtime: Vec<RuntimePermission>,
    pub(crate) install: Vec<InstallPermission>,
    pub(crate) declared: Vec<DeclaredPermission>,
    pub(crate) toggling: Vec<SharedString>,
}

/// The "special app access" settings screens reachable from the options menu,
/// each opened with `am start -a <action>`.
const SPECIAL_ACCESS: [(&str, &str, &str, &str); 9] = [
    (
        "overlay",
        "permissions.special.overlay",
        "icons/shield.svg",
        "android.settings.action.MANAGE_OVERLAY_PERMISSION",
    ),
    (
        "accessibility",
        "permissions.special.accessibility",
        "icons/smartphone.svg",
        "android.settings.ACCESSIBILITY_SETTINGS",
    ),
    (
        "defaultApps",
        "permissions.special.default_apps",
        "icons/apps.svg",
        "android.settings.MANAGE_DEFAULT_APPS_SETTINGS",
    ),
    (
        "writeSettings",
        "permissions.special.write_settings",
        "icons/settings.svg",
        "android.settings.action.MANAGE_WRITE_SETTINGS",
    ),
    (
        "usageAccess",
        "permissions.special.usage_access",
        "icons/gauge.svg",
        "android.settings.USAGE_ACCESS_SETTINGS",
    ),
    (
        "notificationAccess",
        "permissions.special.notification_access",
        "icons/alert.svg",
        "android.settings.ACTION_NOTIFICATION_LISTENER_SETTINGS",
    ),
    (
        "allFilesAccess",
        "permissions.special.all_files_access",
        "icons/folder.svg",
        "android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION",
    ),
    (
        "installUnknownApps",
        "permissions.special.install_unknown",
        "icons/archive.svg",
        "android.settings.MANAGE_UNKNOWN_APP_SOURCES",
    ),
    (
        "doNotDisturb",
        "permissions.special.do_not_disturb",
        "icons/pin.svg",
        "android.settings.NOTIFICATION_POLICY_ACCESS_SETTINGS",
    ),
];

/// Entries in the `runtime permissions:` block. The block is skipped when it
/// is preceded by a `gids=` line (matching the Porpita service), and items
/// must name a permission before the `:`.
fn parse_runtime_permissions(dump: &str) -> Vec<RuntimePermission> {
    let mut out: Vec<RuntimePermission> = Vec::new();
    let mut block = false;
    let mut previous: Option<&str> = None;
    for raw in dump.lines() {
        let line = raw.trim();
        if block {
            if line.is_empty() {
                block = false;
            } else if let Some((name, rest)) = line.split_once(':') {
                let name = name.trim();
                if !name.is_empty() && !name.contains(' ') {
                    upsert_runtime(&mut out, name, rest.contains("granted=true"));
                }
            }
        } else if line == "runtime permissions:" && !previous.is_some_and(|p| p.starts_with("gids="))
        {
            block = true;
        }
        if !line.is_empty() {
            previous = Some(line);
        }
    }
    out
}

/// Entries in the `install permissions:` block; only `android.`/`com.` rows
/// with a grant flag are kept.
fn parse_install_permissions(dump: &str) -> Vec<InstallPermission> {
    let mut out: Vec<InstallPermission> = Vec::new();
    let mut block = false;
    for raw in dump.lines() {
        let line = raw.trim();
        if block {
            if line.is_empty() {
                block = false;
                continue;
            }
            let is_permission = line.starts_with("android.") || line.starts_with("com.");
            if is_permission && let Some((name, rest)) = line.split_once(':') {
                let name = name.trim();
                if !name.is_empty() {
                    let granted = if rest.contains("granted=true") {
                        Some(true)
                    } else if rest.contains("granted=false") {
                        Some(false)
                    } else {
                        None
                    };
                    upsert_install(&mut out, name, granted);
                }
            }
        } else if line == "install permissions:" {
            block = true;
        }
    }
    out
}

/// Entries in the `declared permissions:` block, each carrying the protection
/// level from its `prot=` token.
fn parse_declared_permissions(dump: &str) -> Vec<DeclaredPermission> {
    let mut out: Vec<DeclaredPermission> = Vec::new();
    let mut block = false;
    for raw in dump.lines() {
        let line = raw.trim();
        if block {
            if line.is_empty() {
                block = false;
                continue;
            }
            if let Some((name, rest)) = line.split_once(':') {
                let name = name.trim();
                if !name.is_empty() && name.contains('.') {
                    let protection = rest
                        .split("prot=")
                        .nth(1)
                        .and_then(|value| value.split(',').next().or_else(|| value.split_whitespace().next()))
                        .map(str::trim)
                        .unwrap_or("")
                        .to_string();
                    upsert_declared(&mut out, name, protection);
                }
            }
        } else if line == "declared permissions:" {
            block = true;
        }
    }
    out
}

fn upsert_runtime(out: &mut Vec<RuntimePermission>, name: &str, granted: bool) {
    let replacement = RuntimePermission {
        name: SharedString::from(name),
        granted,
    };
    if let Some(existing) = out.iter_mut().find(|item| item.name.as_ref() == name) {
        *existing = replacement;
    } else {
        out.push(replacement);
    }
}

fn upsert_install(out: &mut Vec<InstallPermission>, name: &str, granted: Option<bool>) {
    let replacement = InstallPermission {
        name: SharedString::from(name),
        granted,
    };
    if let Some(existing) = out.iter_mut().find(|item| item.name.as_ref() == name) {
        *existing = replacement;
    } else {
        out.push(replacement);
    }
}

fn upsert_declared(out: &mut Vec<DeclaredPermission>, name: &str, protection: String) {
    let replacement = DeclaredPermission {
        name: SharedString::from(name),
        protection: SharedString::from(protection),
    };
    if let Some(existing) = out.iter_mut().find(|item| item.name.as_ref() == name) {
        *existing = replacement;
    } else {
        out.push(replacement);
    }
}

impl Hakata {
    pub(crate) fn reset_permissions_state(&mut self) {
        self.permissions_state = PermissionsState::default();
    }

    /// Re-parse the runtime/install/declared lists from a fresh dump and drop
    /// any in-flight toggle markers (the dump is the source of truth).
    pub(crate) fn parse_permissions_into_state(&mut self, raw: &str) {
        self.permissions_state.runtime = parse_runtime_permissions(raw);
        self.permissions_state.install = parse_install_permissions(raw);
        self.permissions_state.declared = parse_declared_permissions(raw);
        self.permissions_state.toggling.clear();
    }

    pub(crate) fn select_permissions_segment(
        &mut self,
        segment: PermissionsSegment,
        cx: &mut Context<Self>,
    ) {
        self.permissions_state.segment = segment;
        self.permissions_menu_open = false;
        cx.notify();
    }

    /// Optimistically flip a runtime permission's grant state, then apply it
    /// with `pm grant`/`pm revoke`. The dump re-fetches on success; a failure
    /// reverts the switch and reports the error.
    pub(crate) fn toggle_runtime_permission(&mut self, name: SharedString, cx: &mut Context<Self>) {
        let Some(serial) = self.selected_device.clone() else {
            self.app_action_status = Some(AppActionStatus::Error {
                message: tr!("common.no_device_selected"),
            });
            cx.notify();
            return;
        };
        let Some(package) = self.selected_package.clone() else {
            return;
        };
        if self.permissions_state.toggling.contains(&name) {
            return;
        }
        let Some(index) = self
            .permissions_state
            .runtime
            .iter()
            .position(|item| item.name == name)
        else {
            return;
        };
        let grant = !self.permissions_state.runtime[index].granted;
        self.permissions_state.toggling.push(name.clone());
        self.permissions_state.runtime[index].granted = grant;
        cx.notify();

        let adb_path = crate::adb::adb_path();
        let serial_for_spawn = serial.clone();
        let package_for_spawn = package.clone();
        let name_for_spawn = name.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    std::process::Command::new(&adb_path)
                        .arg("-s")
                        .arg(serial_for_spawn.as_str())
                        .arg("shell")
                        .arg("pm")
                        .arg(if grant { "grant" } else { "revoke" })
                        .arg(package_for_spawn.as_str())
                        .arg(name_for_spawn.as_str())
                        .output()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.permissions_state.toggling.retain(|item| item != &name);
                let revert = |this: &mut Self, name: &SharedString, granted: bool| {
                    if let Some(item) = this
                        .permissions_state
                        .runtime
                        .iter_mut()
                        .find(|item| &item.name == name)
                    {
                        item.granted = granted;
                    }
                };
                match result {
                    Ok(output) if output.status.success() => {
                        this.fetch_package_dump(true, cx);
                    }
                    Ok(output) => {
                        revert(this, &name, !grant);
                        let message =
                            String::from_utf8_lossy(&output.stderr).trim().to_string();
                        this.app_action_status = Some(AppActionStatus::Error {
                            message: if message.is_empty() {
                                tr!(
                                    if grant { "permissions.grant_failed" } else { "permissions.revoke_failed" },
                                    name = name
                                )
                            } else {
                                message
                            },
                        });
                    }
                    Err(error) => {
                        revert(this, &name, !grant);
                        this.app_action_status = Some(AppActionStatus::Error {
                            message: error.to_string(),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Open an Android settings screen for a special-access topic via
    /// `am start -a <action>`.
    pub(crate) fn start_special_access(&mut self, action: &str, cx: &mut Context<Self>) {
        self.permissions_menu_open = false;
        let Some(serial) = self.selected_device.clone() else {
            self.app_action_status = Some(AppActionStatus::Error {
                message: tr!("common.no_device_selected"),
            });
            cx.notify();
            return;
        };
        let adb_path = crate::adb::adb_path();
        let serial_for_spawn = serial.clone();
        let action_for_spawn = action.to_string();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    std::process::Command::new(&adb_path)
                        .arg("-s")
                        .arg(serial_for_spawn.as_str())
                        .arg("shell")
                        .arg("am")
                        .arg("start")
                        .arg("-a")
                        .arg(&action_for_spawn)
                        .output()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(output) if output.status.success() => {
                        this.app_action_status = Some(AppActionStatus::Done {
                            message: tr!("permissions.opened_settings"),
                        });
                    }
                    Ok(output) => {
                        let message =
                            String::from_utf8_lossy(&output.stderr).trim().to_string();
                        this.app_action_status = Some(AppActionStatus::Error {
                            message: if message.is_empty() {
                                tr!("permissions.open_settings_failed")
                            } else {
                                message
                            },
                        });
                    }
                    Err(error) => {
                        this.app_action_status = Some(AppActionStatus::Error {
                            message: error.to_string(),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Open the package's own App Info screen.
    pub(crate) fn open_app_info(&mut self, cx: &mut Context<Self>) {
        self.permissions_menu_open = false;
        let Some(serial) = self.selected_device.clone() else {
            self.app_action_status = Some(AppActionStatus::Error {
                message: tr!("common.no_device_selected"),
            });
            cx.notify();
            return;
        };
        let Some(package) = self.selected_package.clone() else {
            return;
        };
        let adb_path = crate::adb::adb_path();
        let serial_for_spawn = serial.clone();
        let package_for_spawn = package.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    std::process::Command::new(&adb_path)
                        .arg("-s")
                        .arg(serial_for_spawn.as_str())
                        .arg("shell")
                        .arg("am")
                        .arg("start")
                        .arg("-a")
                        .arg("android.settings.APPLICATION_DETAILS_SETTINGS")
                        .arg("-d")
                        .arg(format!("package:{}", package_for_spawn.as_str()))
                        .output()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(output) if output.status.success() => {
                        this.app_action_status = Some(AppActionStatus::Done {
                            message: tr!("permissions.opened_app_info", package = package),
                        });
                    }
                    Ok(output) => {
                        let message =
                            String::from_utf8_lossy(&output.stderr).trim().to_string();
                        this.app_action_status = Some(AppActionStatus::Error {
                            message: if message.is_empty() {
                                tr!("permissions.open_app_info_failed")
                            } else {
                                message
                            },
                        });
                    }
                    Err(error) => {
                        this.app_action_status = Some(AppActionStatus::Error {
                            message: error.to_string(),
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The body of the Permissions tab: a segmented switcher (Runtime /
    /// Install / Declared / Requested), a per-segment search, and an options
    /// menu with Refresh, App Info and the special-access screens.
    pub(crate) fn render_permissions_tab(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        if self.package_dump_loading {
            return self.render_permissions_loading(&theme);
        }
        if let Some(error) = &self.package_dump_error {
            return self.render_permissions_error(&theme, error, cx);
        }
        if self.package_dump_raw.is_none() {
            return self.render_permissions_center(&theme, &tr!("apps.no_app_selected"));
        }

        let mut column = div().size_full().flex().flex_col().gap(px(8.0));
        column = column
            .child(self.render_segment_tabs(&theme, cx))
            .child(self.render_permissions_segment(window, cx));

        let root = div()
            .id("apps-permissions")
            .size_full()
            .relative()
            .child(column);
        if self.permissions_menu_open
            && let Some(bounds) = self.permissions_menu_bounds.get()
        {
            return root
                .child(self.render_options_menu(bounds, cx))
                .into_any_element();
        }
        root.into_any_element()
    }

    /// The four segment tabs, styled like the apps detail tab bar.
    fn render_segment_tabs(&self, theme: &Theme, cx: &mut Context<Self>) -> Div {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .children(PermissionsSegment::ALL.iter().map(|segment| {
                let selected = self.permissions_state.segment == *segment;
                div()
                    .id(SharedString::from(format!(
                        "apps-permissions-segment-{}",
                        segment.label().to_lowercase()
                    )))
                    .px(px(10.0))
                    .h(px(26.0))
                    .flex_none()
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .cursor_default()
                    .when(selected, |element| element.bg(theme.overlay))
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .text_size(px(12.0))
                    .text_color(if selected {
                        theme.text
                    } else {
                        theme.text_secondary
                    })
                    .child(segment.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_permissions_segment(*segment, cx);
                    }))
            }))
            .child(
                div()
                    .flex_1()
                    .child(div()),
            )
            .child(self.render_options_trigger(theme, cx))
    }

    /// The `⋮` button that opens the options menu.
    fn render_options_trigger(&self, theme: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let bounds_ref = self.permissions_menu_bounds.clone();
        div()
            .id("apps-permissions-options")
            .relative()
            .tab_index(0)
            .size(px(24.0))
            .flex_none()
            .rounded(px(6.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .focus_visible(|style| style.bg(theme.overlay))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(
                canvas(
                    move |probe: Bounds<Pixels>, _, _| bounds_ref.set(Some(probe)),
                    |_, _, _, _| (),
                )
                .absolute()
                .inset_0(),
            )
            .child(icon("icons/more-vertical.svg", 14.0, theme.text_secondary))
            .on_click(cx.listener(|this, _, _, cx| {
                this.permissions_menu_open = !this.permissions_menu_open;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.permissions_menu_open = !this.permissions_menu_open;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
    }

    /// The options dropdown: Refresh, App Info, then the special-access list.
    fn render_options_menu(&self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        self.render_dropdown_card(
            cx,
            "apps-permissions-options-menu",
            bounds,
            self.permissions_menu_bounds.clone(),
            close_options_menu,
            232.0,
            |theme, cx| {
                let mut card = div().flex().flex_col().gap(px(1.0));
                card = card.child(menu_row(
                    theme,
                    SharedString::from("apps-permissions-refresh"),
                    "icons/refresh-cw.svg",
                    tr!("common.refresh"),
                    |this, cx| {
                        this.fetch_package_dump(true, cx);
                    },
                    cx,
                ));
                card = card.child(menu_row(
                    theme,
                    SharedString::from("apps-permissions-app-info"),
                    "icons/info.svg",
                    tr!("permissions.app_info"),
                    |this, cx| {
                        this.open_app_info(cx);
                    },
                    cx,
                ));
                card = card.child(
                    div()
                        .h(px(1.0))
                        .mx(px(8.0))
                        .my(px(4.0))
                        .bg(theme.border),
                );
                card = card.child(
                    div()
                        .px(px(8.0))
                        .pt(px(2.0))
                        .pb(px(4.0))
                        .text_size(px(10.0))
                        .text_color(theme.text_tertiary)
                        .child(tr_cow!("permissions.special_access")),
                );
                for (id, label_key, icon_path, action) in SPECIAL_ACCESS {
                    card = card.child(menu_row(
                        theme,
                        SharedString::from(format!("apps-permissions-special-{}", id)),
                        icon_path,
                        tr!(label_key),
                        move |this, cx| {
                            this.start_special_access(action, cx);
                        },
                        cx,
                    ));
                }
                card
            },
        )
    }

    /// The toolbar (search plus, for Runtime, the bulk grant/revoke buttons)
    /// and the rows for the active segment.
    fn render_permissions_segment(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let segment = self.permissions_state.segment;
        let toolbar = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(div().flex_1().min_w_0().child(self.render_permissions_search(window, cx)))
            .when(
                segment == PermissionsSegment::Runtime,
                |element| {
                    let package = self.selected_package.clone();
                    element
                        .child(bulk_button(
                            &theme,
                            "apps-permissions-grant-all",
                            "icons/check.svg",
                            tr!("permissions.grant_all"),
                            package.clone(),
                            true,
                            cx,
                        ))
                        .child(bulk_button(
                            &theme,
                            "apps-permissions-revoke-all",
                            "icons/x.svg",
                            tr!("permissions.revoke_all"),
                            package,
                            false,
                            cx,
                        ))
                },
            );

        let rows = match segment {
            PermissionsSegment::Runtime => self.render_runtime_rows(&theme, cx),
            PermissionsSegment::Install => self.render_install_rows(&theme, cx),
            PermissionsSegment::Declared => self.render_declared_rows(&theme, cx),
            PermissionsSegment::Requested => self.render_requested_rows(&theme, cx),
        };

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(toolbar)
            .child(rows)
            .into_any_element()
    }

    /// The permissions search box: the same one-line field used for apps.
    fn render_permissions_search(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let focused = self.permissions_search.read(cx).is_visually_focused(window);
        let has_content = !self.permissions_search.read(cx).is_empty();

        let mut shell = div()
            .id("apps-permissions-search")
            .h(px(28.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(if focused {
                theme.accent
            } else {
                theme.border_strong
            })
            .bg(theme.inset)
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(px(11.5))
            .line_height(px(16.0))
            .child(icon("icons/search.svg", 13.0, theme.text_tertiary))
            .child(div().min_w_0().flex_1().child(self.permissions_search.clone()));

        if has_content {
            shell = shell.child(
                div()
                    .id("apps-permissions-search-clear")
                    .tab_index(0)
                    .size(px(18.0))
                    .flex_none()
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .focus_visible(|style| style.bg(theme.overlay))
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .child(icon("icons/x.svg", 11.0, theme.text_ghost))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let field = this.permissions_search.clone();
                        field.update(cx, |field, cx| {
                            field.set_content("", cx);
                            field.select_range(0..0, cx);
                        });
                        window.focus(&field.read(cx).focus(), cx);
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            let field = this.permissions_search.clone();
                            field.update(cx, |field, cx| {
                                field.set_content("", cx);
                                field.select_range(0..0, cx);
                            });
                            window.focus(&field.read(cx).focus(), cx);
                            cx.stop_propagation();
                        }
                    })),
            );
        }

        shell.into_any_element()
    }

    fn permissions_query(&self, cx: &mut Context<Self>) -> String {
        self.permissions_search
            .read(cx)
            .content()
            .trim()
            .to_lowercase()
    }

    fn render_runtime_rows(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let query = self.permissions_query(cx);
        let mut rows = div().flex().flex_col().gap(px(2.0));
        let mut shown = 0usize;
        for permission in &self.permissions_state.runtime {
            let name = permission.name.to_lowercase();
            if !query.is_empty() && !name.contains(&query) {
                continue;
            }
            rows = rows.child(self.render_runtime_row(theme, permission, cx));
            shown += 1;
        }
        if shown == 0 {
            return self.render_permissions_center(theme, &tr!("permissions.no_runtime"));
        }
        rows.into_any_element()
    }

    fn render_runtime_row(
        &self,
        theme: &Theme,
        permission: &RuntimePermission,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let name = permission.name.clone();
        let click_name = name.clone();
        let key_name = name.clone();
        let granted = permission.granted;
        let toggling = self.permissions_state.toggling.contains(&name);
        let switch = div()
            .id(SharedString::from(format!("apps-permission-toggle-{}", name)))
            .tab_index(0)
            .flex_none()
            .w(px(30.0))
            .h(px(18.0))
            .rounded(px(9.0))
            .bg(if granted {
                theme.accent
            } else {
                theme.overlay_strong
            })
            .opacity(if toggling { 0.55 } else { 1.0 })
            .flex()
            .items_center()
            .px(px(2.0))
            .when(granted, |element| element.justify_end())
            .cursor_default()
            .focus_visible(|style| style.bg(theme.accent))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_runtime_permission(click_name.clone(), cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.toggle_runtime_permission(key_name.clone(), cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .size(px(14.0))
                    .rounded(px(7.0))
                    .bg(theme.surface),
            );
        div()
            .id(SharedString::from(format!("apps-permission-runtime-{}", name)))
            .h(px(30.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .child(icon("icons/shield.svg", 13.0, theme.text_ghost))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.5))
                    .text_color(theme.text)
                    .child(name.clone()),
            )
            .child(switch)
    }

    fn render_install_rows(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let query = self.permissions_query(cx);
        let mut rows = div().flex().flex_col().gap(px(2.0));
        let mut shown = 0usize;
        for permission in &self.permissions_state.install {
            let name = permission.name.to_lowercase();
            if !query.is_empty() && !name.contains(&query) {
                continue;
            }
            rows = rows.child(self.render_install_row(theme, permission));
            shown += 1;
        }
        if shown == 0 {
            return self.render_permissions_center(theme, &tr!("permissions.no_install"));
        }
        rows.into_any_element()
    }

    fn render_install_row(&self, theme: &Theme, permission: &InstallPermission) -> Stateful<Div> {
        let name = permission.name.clone();
        let (label, color) = match permission.granted {
            Some(true) => (tr!("permissions.granted"), theme.success),
            Some(false) => (tr!("permissions.denied"), theme.danger),
            None => (tr!("permissions.unknown"), theme.text_tertiary),
        };
        div()
            .id(SharedString::from(format!("apps-permission-install-{}", name)))
            .h(px(30.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .child(icon("icons/shield.svg", 13.0, theme.text_ghost))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.5))
                    .text_color(theme.text)
                    .child(name.clone()),
            )
            .child(
                div()
                    .h(px(18.0))
                    .px(px(8.0))
                    .rounded(px(9.0))
                    .bg(theme.overlay_strong)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.0))
                    .text_color(color)
                    .child(label),
            )
    }

    fn render_declared_rows(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let query = self.permissions_query(cx);
        let mut rows = div().flex().flex_col().gap(px(2.0));
        let mut shown = 0usize;
        for permission in &self.permissions_state.declared {
            let name = permission.name.to_lowercase();
            if !query.is_empty() && !name.contains(&query) {
                continue;
            }
            rows = rows.child(self.render_declared_row(theme, permission));
            shown += 1;
        }
        if shown == 0 {
            return self.render_permissions_center(theme, &tr!("permissions.no_declared"));
        }
        rows.into_any_element()
    }

    fn render_declared_row(&self, theme: &Theme, permission: &DeclaredPermission) -> Stateful<Div> {
        let name = permission.name.clone();
        let protection = permission.protection.clone();
        let color = if protection == "dangerous" {
            theme.danger
        } else if protection == "signature" {
            theme.accent
        } else {
            theme.text_tertiary
        };
        div()
            .id(SharedString::from(format!("apps-permission-declared-{}", name)))
            .h(px(30.0))
            .px(px(8.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .hover(|element| element.bg(theme.overlay))
            .child(icon("icons/shield.svg", 13.0, theme.text_ghost))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.5))
                    .text_color(theme.text)
                    .child(name.clone()),
            )
            .child(
                div()
                    .h(px(18.0))
                    .px(px(8.0))
                    .rounded(px(9.0))
                    .bg(theme.overlay_strong)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.0))
                    .text_color(color)
                    .child(protection),
            )
    }

    fn render_requested_rows(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let query = self.permissions_query(cx);
        let mut rows = div().flex().flex_col().gap(px(2.0));
        let mut shown = 0usize;
        for permission in &self.permissions {
            let name = permission.to_lowercase();
            if !query.is_empty() && !name.contains(&query) {
                continue;
            }
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("apps-permission-requested-{}", permission)))
                    .h(px(28.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .cursor_default()
                    .hover(|element| element.bg(theme.overlay))
                    .child(icon("icons/shield.svg", 13.0, theme.text_ghost))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(theme.text_secondary)
                            .child(permission.clone()),
                    ),
            );
            shown += 1;
        }
        if shown == 0 {
            return self.render_permissions_center(theme, &tr!("permissions.no_requested"));
        }
        rows.into_any_element()
    }

    fn render_permissions_center(&self, theme: &Theme, message: &str) -> AnyElement {
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

    fn render_permissions_loading(&self, theme: &Theme) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .child(
                icon("icons/loader-circle.svg", 14.0, theme.text_tertiary).with_animation(
                    SharedString::from("apps-permissions-loading-spinner"),
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
                    .child(tr_cow!("permissions.loading")),
            )
            .into_any_element()
    }

    fn render_permissions_error(&self, theme: &Theme, message: &str, cx: &mut Context<Self>) -> AnyElement {
        let retry = div()
            .id("apps-permissions-retry")
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
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.fetch_package_dump(true, cx);
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

fn close_options_menu(this: &mut Hakata, cx: &mut Context<Hakata>) {
    this.permissions_menu_open = false;
    cx.notify();
}

/// A single options-menu row: icon, label, hover/active states and both a
/// click and keyboard activation that run the same action.
fn menu_row<F>(
    theme: &Theme,
    id: SharedString,
    icon_path: &'static str,
    label: String,
    action: F,
    cx: &mut Context<Hakata>,
) -> Stateful<Div>
where
    F: Clone + Fn(&mut Hakata, &mut Context<Hakata>) + 'static,
{
    let click_action = action.clone();
    div()
        .id(id)
        .tab_index(0)
        .px(px(8.0))
        .h(px(28.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_default()
        .focus_visible(|style| style.bg(theme.overlay))
        .hover(|element| element.bg(theme.overlay))
        .active(|element| element.bg(theme.overlay_strong))
        .on_click(cx.listener(move |this, _, _, cx| {
            click_action(this, cx);
        }))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                action(this, cx);
                cx.stop_propagation();
            }
        }))
        .child(icon(icon_path, 13.0, theme.text_secondary))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(11.5))
                .text_color(theme.text_secondary)
                .child(label),
        )
}

/// The Grant all / Revoke all header buttons for the Runtime segment.
fn bulk_button(
    theme: &Theme,
    id: &'static str,
    icon_path: &'static str,
    label: String,
    package: Option<SharedString>,
    grant: bool,
    cx: &mut Context<Hakata>,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(22.0))
        .px(px(8.0))
        .rounded(px(5.0))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(5.0))
        .cursor_default()
        .hover(|element| element.bg(theme.overlay))
        .active(|element| element.bg(theme.overlay_strong))
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(package) = &package {
                this.start_permission_run(package.clone(), grant, cx);
            }
        }))
        .child(icon(icon_path, 11.0, theme.text_secondary))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(theme.text_secondary)
                .child(label),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = "\
Package [com.example.app] (abcdef):
  requested permissions:
    android.permission.CAMERA

  install permissions:
    android.permission.CAMERA: granted=true, flags=[ PRIVILEGED ]
    android.permission.WRITE_EXTERNAL_STORAGE: granted=false, flags=[ REVOKE_ON_UPGRADE ]
    com.example.app.THEME: granted=true

  User 0:
    runtime permissions:
      android.permission.CAMERA: granted=true, flags=[ USER_SET ]
      android.permission.RECORD_AUDIO: granted=false, flags=[ USER_SET ]

  declared permissions:
    android.permission.CAMERA: prot=dangerous, CORE
    com.example.app.INTERNAL: prot=signature
    com.example.app.SECRET: prot=normal
";

    #[test]
    fn parses_runtime_permissions() {
        let permissions = parse_runtime_permissions(DUMP);
        let names: Vec<&str> = permissions.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, vec!["android.permission.CAMERA", "android.permission.RECORD_AUDIO"]);
        assert!(permissions[0].granted);
        assert!(!permissions[1].granted);
    }

    #[test]
    fn skips_runtime_block_after_gids() {
        let dump = "gids=[1, 2]\nruntime permissions:\n  android.permission.CAMERA: granted=true\n";
        assert!(parse_runtime_permissions(dump).is_empty());
    }

    #[test]
    fn parses_install_permissions() {
        let permissions = parse_install_permissions(DUMP);
        let names: Vec<&str> = permissions.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "android.permission.CAMERA",
                "android.permission.WRITE_EXTERNAL_STORAGE",
                "com.example.app.THEME"
            ]
        );
        assert_eq!(permissions[0].granted, Some(true));
        assert_eq!(permissions[1].granted, Some(false));
        assert_eq!(permissions[2].granted, Some(true));
    }

    #[test]
    fn parses_declared_permissions() {
        let permissions = parse_declared_permissions(DUMP);
        let names: Vec<&str> = permissions.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "android.permission.CAMERA",
                "com.example.app.INTERNAL",
                "com.example.app.SECRET"
            ]
        );
        let levels: Vec<&str> = permissions
            .iter()
            .map(|item| item.protection.as_str())
            .collect();
        assert_eq!(levels, vec!["dangerous", "signature", "normal"]);
    }
}
