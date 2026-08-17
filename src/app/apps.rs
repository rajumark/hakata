use std::time::Duration;

use gpui::{
    actions, Animation, AnimationExt, AnyElement, App, Bounds, ClipboardItem, Context, Div,
    InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, MouseButton, ObjectFit,
    ParentElement, Pixels, SharedString, Stateful, Styled, Window, canvas, div, img, px,
    prelude::*,
};

use crate::adb::AppFilter;
use crate::theme::Theme;

use super::{Hakata, PanelResizeSide, PanelResizeTarget, icon, package_info};

actions!(apps_title, [CopyPackageTitle]);

/// Bind the title row's copy key. Called once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-c", CopyPackageTitle, Some("AppsTitle"))]);
}

/// The sub-tabs shown on the Apps detail pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppsTab {
    Overview,
    Permissions,
    Paths,
    Files,
}

impl AppsTab {
    pub(crate) const ALL: [Self; 4] = [Self::Overview, Self::Permissions, Self::Paths, Self::Files];

    pub(crate) fn label(self) -> String {
        match self {
            Self::Overview => tr!("apps.tab.overview"),
            Self::Permissions => tr!("apps.tab.permissions"),
            Self::Paths => tr!("apps.tab.paths"),
            Self::Files => tr!("apps.tab.files"),
        }
    }
}

impl Hakata {
    /// Fetch third-party packages for the selected device on the background
    /// executor. `force` bypasses the "already fresh for this device" guard so
    /// the refresh button always hits adb. A generation counter drops results
    /// from a superseded run (device switched mid-flight).
    pub(crate) fn refresh_packages(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(serial) = self.selected_device.clone() else {
            self.packages.clear();
            self.packages_loading = false;
            self.packages_loaded = false;
            self.packages_device = None;
            self.packages_error = None;
            self.app_icons.clear();
            cx.notify();
            return;
        };
        if !force
            && self.packages_device.as_deref() == Some(serial.as_str())
            && self.packages_loaded
        {
            self.fetch_app_icons(cx);
            return;
        }
        let adb_path = crate::adb::adb_path();
        if !crate::adb::is_installed() {
            self.packages.clear();
            self.packages_loading = false;
            self.packages_loaded = false;
            self.packages_device = Some(serial);
            self.packages_error = None;
            cx.notify();
            return;
        }
        self.packages_refresh_epoch += 1;
        let epoch = self.packages_refresh_epoch;
        let serial_for_spawn = serial.clone();
        let filter = self.apps_filter;
        self.packages_loading = true;
        self.packages_device = Some(serial);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    std::process::Command::new(&adb_path)
                        .arg("-s")
                        .arg(serial_for_spawn.as_str())
                        .arg("shell")
                        .arg("pm")
                        .arg("list")
                        .arg("packages")
                        .args(filter.adb_flag())
                        .output()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.packages_refresh_epoch != epoch {
                    return;
                }
                this.packages_loading = false;
                this.packages_loaded = true;
                this.packages_error = None;
                match result {
                    Ok(output) if output.status.success() => {
                        this.packages = crate::adb::parse_packages(
                            &String::from_utf8_lossy(&output.stdout),
                        )
                        .into_iter()
                        .map(SharedString::from)
                        .collect();
                    }
                    Ok(output) => {
                        this.packages.clear();
                        this.packages_error = Some(
                            String::from_utf8_lossy(&output.stderr).trim().to_string(),
                        );
                    }
                    Err(error) => {
                        this.packages.clear();
                        this.packages_error = Some(error.to_string());
                    }
                }
                cx.notify();
                this.fetch_app_icons(cx);
            });
        })
        .detach();
    }

    /// Start the once-only 5-second background loop that silently re-queries
    /// the package list while the Apps page is visible. The loop itself keeps
    /// running, but it does no work (and never touches the UI) while another
    /// page is selected, so leaving the Apps page effectively pauses it and
    /// coming back resumes it — the immediate poll on page entry covers the
    /// gap.
    pub(crate) fn ensure_apps_refresh_loop(&mut self, cx: &mut Context<Self>) {
        if self.apps_refresh_started {
            return;
        }
        self.apps_refresh_started = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(super::APPS_REFRESH_INTERVAL)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.selected_page == super::MenuPage::Apps
                        && this.selected_device.is_some()
                    {
                        this.poll_packages(cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Silently re-query `pm list packages` on the background executor and
    /// only re-render when the result actually differs from what's on screen.
    /// The loading state is shown only while nothing is cached yet, so a quiet
    /// poll never flickers. A generation counter drops a superseded result.
    pub(crate) fn poll_packages(&mut self, cx: &mut Context<Self>) {
        if self.packages_polling {
            return;
        }
        let Some(serial) = self.selected_device.clone() else {
            self.packages.clear();
            self.packages_loading = false;
            self.packages_loaded = false;
            self.packages_device = None;
            self.packages_error = None;
            self.app_icons.clear();
            cx.notify();
            return;
        };
        let adb_path = crate::adb::adb_path();
        if !crate::adb::is_installed() {
            self.packages.clear();
            self.packages_loading = false;
            self.packages_loaded = false;
            self.packages_device = Some(serial);
            self.packages_error = None;
            self.app_icons.clear();
            cx.notify();
            return;
        }
        let show_loading =
            !self.packages_loaded || self.packages_device.as_deref() != Some(serial.as_str());
        if show_loading {
            self.packages_loading = true;
            cx.notify();
        }
        self.packages_polling = true;
        self.packages_refresh_epoch += 1;
        let epoch = self.packages_refresh_epoch;
        let serial_for_spawn = serial.clone();
        let filter = self.apps_filter;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    std::process::Command::new(&adb_path)
                        .arg("-s")
                        .arg(serial_for_spawn.as_str())
                        .arg("shell")
                        .arg("pm")
                        .arg("list")
                        .arg("packages")
                        .args(filter.adb_flag())
                        .output()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.packages_polling = false;
                if this.packages_refresh_epoch != epoch {
                    return;
                }
                let mut next_packages = Vec::new();
                let mut next_error = None;
                match result {
                    Ok(output) if output.status.success() => {
                        next_packages = crate::adb::parse_packages(
                            &String::from_utf8_lossy(&output.stdout),
                        )
                        .into_iter()
                        .map(SharedString::from)
                        .collect();
                    }
                    Ok(output) => {
                        next_error =
                            Some(String::from_utf8_lossy(&output.stderr).trim().to_string());
                    }
                    Err(error) => {
                        next_error = Some(error.to_string());
                    }
                }
                let was_loading = this.packages_loading;
                let device_changed = this.packages_device.as_deref() != Some(serial.as_str());
                let list_changed =
                    this.packages != next_packages || this.packages_error != next_error;
                let changed = was_loading || device_changed || list_changed;
                this.packages_loading = false;
                this.packages_loaded = true;
                this.packages_device = Some(serial);
                this.packages_error = next_error;
                if changed {
                    this.packages = next_packages;
                    cx.notify();
                    this.fetch_app_icons(cx);
                }
            });
        })
        .detach();
    }

    /// Fetch PNG icons for the selected device's packages on the background
    /// executor, storing the local paths under `<device-id> -> <package>`.
    /// Packages already cached in memory are skipped; the on-disk cache under
    /// the app-support dir makes repeat fetches cheap. A generation counter
    /// drops results from a superseded run (device switched mid-flight).
    pub(crate) fn fetch_app_icons(&mut self, cx: &mut Context<Self>) {
        if self.app_icons_fetching {
            return;
        }
        let Some(serial) = self.selected_device.clone() else {
            self.app_icons.clear();
            cx.notify();
            return;
        };
        if !crate::adb::is_installed() || self.packages.is_empty() {
            return;
        }
        let uncached: Vec<String> = self
            .packages
            .iter()
            .filter(|package| {
                !self
                    .app_icons
                    .get(serial.as_str())
                    .is_some_and(|icons| icons.contains_key(package.as_str()))
            })
            .map(|package| package.to_string())
            .collect();
        if uncached.is_empty() {
            return;
        }
        self.app_icons_epoch += 1;
        let epoch = self.app_icons_epoch;
        let serial_for_spawn = serial.clone();
        self.app_icons_fetching = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { crate::app_icons::fetch_icons(&serial_for_spawn, &uncached) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.app_icons_fetching = false;
                if this.app_icons_epoch != epoch {
                    return;
                }
                if let Ok(found) = result {
                    let icons = this.app_icons.entry(serial.clone()).or_default();
                    for (package, path) in found {
                        icons.insert(SharedString::from(package), path);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Fetch `adb -s <device> shell dumpsys package <pkg>` for the selected
    /// package and stash the raw output on the entity. The dump is kept until
    /// a different package (or device) is selected; parsing happens on demand
    /// per tab. `force` bypasses the cache guard so actions that change
    /// package state (enable/disable/clear-data/grant) always re-read. A
    /// generation counter drops a superseded result.
    pub(crate) fn fetch_package_dump(
        &mut self,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        self.fetch_package_paths(false, cx);
        let Some(serial) = self.selected_device.clone() else {
            self.package_dump_raw = None;
            self.package_dump_loading = false;
            self.package_dump_error = None;
            self.permissions.clear();
            self.reset_permissions_state();
            cx.notify();
            return;
        };
        let Some(package) = self.selected_package.clone() else {
            self.package_dump_raw = None;
            self.package_dump_loading = false;
            self.package_dump_error = None;
            self.permissions.clear();
            self.reset_permissions_state();
            cx.notify();
            return;
        };
        if !force
            && self.package_dump_device.as_deref() == Some(serial.as_str())
            && self.package_dump_package.as_deref() == Some(package.as_str())
            && self.package_dump_raw.is_some()
        {
            return;
        }
        let adb_path = crate::adb::adb_path();
        if !crate::adb::is_installed() {
            self.package_dump_raw = None;
            self.package_dump_loading = false;
            self.package_dump_error = None;
            self.permissions.clear();
            self.reset_permissions_state();
            cx.notify();
            return;
        }
        self.package_dump_epoch += 1;
        let epoch = self.package_dump_epoch;
        let serial_for_spawn = serial.clone();
        let package_for_spawn = package.clone();
        self.package_dump_loading = true;
        self.package_dump_device = Some(serial);
        self.package_dump_package = Some(package);
        self.package_dump_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    std::process::Command::new(&adb_path)
                        .arg("-s")
                        .arg(serial_for_spawn.as_str())
                        .arg("shell")
                        .arg("dumpsys")
                        .arg("package")
                        .arg(package_for_spawn.as_str())
                        .output()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.package_dump_epoch != epoch {
                    return;
                }
                this.package_dump_loading = false;
                match result {
                    Ok(output) if output.status.success() => {
                        let raw = SharedString::from(String::from_utf8_lossy(&output.stdout));
                        this.package_dump_raw = Some(raw.clone());
                        this.permissions = package_info::parse_requested_permissions(&raw)
                            .into_iter()
                            .map(SharedString::from)
                            .collect();
                        this.parse_permissions_into_state(&raw);
                        this.package_dump_error = None;
                    }
                    Ok(output) => {
                        this.package_dump_raw = None;
                        this.package_dump_error = Some(
                            String::from_utf8_lossy(&output.stderr).trim().to_string(),
                        );
                    }
                    Err(error) => {
                        this.package_dump_raw = None;
                        this.package_dump_error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn filtered_packages(&self, cx: &App) -> Vec<SharedString> {
        let query = self.apps_search.read(cx).content().trim().to_lowercase();
        if query.is_empty() {
            return self.packages.clone();
        }
        self.packages
            .iter()
            .filter(|package| package.to_lowercase().contains(&query))
            .cloned()
            .collect()
    }

    pub(crate) fn render_apps_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let panel_width = self.effective_apps_panel_width(window);
        let has_selection = self.selected_package.is_some();

        let left_panel = div()
            .h_full()
            .relative()
            .flex()
            .flex_col()
            .px(px(12.0))
            .py(px(10.0))
            .when(has_selection, |element| {
                element
                    .w(px(panel_width))
                    .flex_none()
                    .border_r_1()
                    .border_color(theme.sidebar_border)
                    .child(self.render_panel_resize_handle(
                        "apps-resize-handle",
                        PanelResizeTarget::Apps,
                        PanelResizeSide::Right,
                        cx,
                    ))
            })
            .when(!has_selection, |element| element.flex_1())
            .child(self.render_apps_search(window, cx))
            .child(div().h(px(8.0)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(self.render_apps_list(cx)),
            );

        let tab_bar = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .py(px(2.0))
            .children(AppsTab::ALL.iter().map(|tab| {
                let selected = self.selected_apps_tab == *tab;
                div()
                    .id(SharedString::from(format!("apps-tab-{}", tab.label().to_lowercase())))
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
                    .child(SharedString::from(tab.label()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.selected_apps_tab != *tab {
                            this.selected_apps_tab = *tab;
                            cx.notify();
                        }
                    }))
            }));

        let title_row = div()
            .id("apps-title")
            .group("apps-title-row")
            .flex_none()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.0))
            .key_context("AppsTitle")
            .track_focus(&self.apps_title_focus)
            .tab_index(0)
            .on_action(cx.listener(|this, _: &CopyPackageTitle, _, cx| {
                if let Some(package) = this.selected_package.clone() {
                    cx.write_to_clipboard(ClipboardItem::new_string(package.to_string()));
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.apps_title_focus, cx);
                    if !this.title_selected {
                        this.title_selected = true;
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if this.title_selected {
                    this.title_selected = false;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .flex_none()
                    .min_w_0()
                    .truncate()
                    .text_size(px(15.0))
                    .text_color(theme.text)
                    .when(self.title_selected, |element| {
                        element.rounded(px(4.0)).bg(theme.overlay_strong)
                    })
                    .child(self.selected_package.clone().unwrap_or_else(|| {
                        tr_cow!("apps.no_app_selected").into()
                    })),
            )
            .child(
                div()
                    .id("apps-title-copy")
                    .tab_index(0)
                    .size(px(22.0))
                    .flex_none()
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_default()
                    .opacity(0.0)
                    .group_hover("apps-title-row", |element| element.opacity(1.0))
                    .focus_visible(|style| style.bg(theme.overlay))
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
                    .child(icon("icons/copy.svg", 12.0, theme.text_tertiary))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(package) = this.selected_package.clone() {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                package.to_string(),
                            ));
                        }
                    }))
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            if let Some(package) = this.selected_package.clone() {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    package.to_string(),
                                ));
                            }
                            cx.stop_propagation();
                        }
                    })),
            );

        let right_panel = div()
            .flex_1()
            .h_full()
            .min_w_0()
            .flex()
            .flex_col()
            .pt(px(14.0))
            .px(px(16.0))
            .child(title_row)
            .child(div().h(px(6.0)))
            .child(tab_bar)
            .child(div().h(px(10.0)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(self.render_apps_tab_content(window, cx)),
            );

        div()
            .id("apps-page")
            .size_full()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(left_panel)
                    .when(has_selection, |element| element.child(right_panel)),
            )
            .child(self.render_action_status(cx))
            .into_any_element()
    }

    /// The body for the selected Apps detail tab. Overview shows the parsed
    /// `dumpsys` facts; the other tabs are placeholders for now.
    fn render_apps_tab_content(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        match self.selected_apps_tab {
            AppsTab::Overview => self.render_overview_tab(cx),
            AppsTab::Permissions => self.render_permissions_tab(window, cx),
            AppsTab::Paths => self.render_paths_tab(cx),
            tab => self.render_tab_placeholder(tab, cx),
        }
    }

    fn render_tab_placeholder(&self, tab: AppsTab, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(theme.text_secondary)
                    .child(SharedString::from(tab.label())),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_ghost)
                    .child(tr_cow!("common.coming_soon")),
            )
            .into_any_element()
    }

    /// Basic information about the selected package, parsed from the cached
    /// `dumpsys package` dump. Returns `None` while loading or when the dump
    /// is missing.
    fn parsed_package_info(&self) -> Option<package_info::PackageInfo> {
        self.package_dump_raw
            .as_deref()
            .map(package_info::parse_package_info)
    }

    fn render_overview_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        if self.package_dump_loading {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(
                    icon("icons/loader-circle.svg", 14.0, theme.text_tertiary).with_animation(
                        SharedString::from("apps-overview-loading-spinner"),
                        Animation::new(Duration::from_millis(900))
                            .repeat()
                            .with_easing(gpui::linear),
                        |icon, delta| {
                            icon.with_transformation(gpui::Transformation::rotate(
                                gpui::percentage(delta),
                            ))
                        },
                    ),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.text_tertiary)
                        .child(tr_cow!("apps.loading_package_info")),
                )
                .into_any_element();
        }
        if let Some(error) = &self.package_dump_error {
            return self.render_overview_center(theme.text_ghost, error.clone(), cx);
        }
        let Some(info) = self.parsed_package_info() else {
            return self.render_overview_center(theme.text_ghost, tr!("apps.no_package_info"), cx);
        };

        let fields: Vec<(String, String)> = vec![
            (tr!("apps.version_name"), info.version_name.unwrap_or_default()),
            (tr!("apps.version_code"), info.version_code.unwrap_or_default()),
            (tr!("apps.target_sdk"), info.target_sdk.unwrap_or_default()),
            (tr!("apps.min_sdk"), info.min_sdk.unwrap_or_default()),
            (tr!("apps.uid"), info.uid.unwrap_or_default()),
            (
                tr!("apps.first_install"),
                info.first_install_time.unwrap_or_default(),
            ),
            (
                tr!("apps.last_update"),
                info.last_update_time.unwrap_or_default(),
            ),
            (tr!("apps.data_dir"), info.data_dir.unwrap_or_default()),
            (tr!("apps.code_path"), info.code_path.unwrap_or_default()),
            (
                tr!("apps.flags"),
                if info.flags.is_empty() {
                    String::new()
                } else {
                    info.flags.join(", ")
                },
            ),
        ];

        let mut rows = div().flex().flex_col();
        for (label, value) in fields {
            let value = if value.is_empty() {
                "—".to_string()
            } else {
                value
            };
            rows = rows.child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(12.0))
                    .py(px(3.0))
                    .child(
                        div()
                            .w(px(96.0))
                            .flex_none()
                            .text_size(px(11.5))
                            .text_color(theme.text_tertiary)
                            .truncate()
                            .child(SharedString::from(label)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(theme.text)
                            .child(SharedString::from(value)),
                    ),
            );
        }

        div()
            .id("apps-overview")
            .size_full()
            .overflow_y_scroll()
            .py(px(8.0))
            .child(rows)
            .into_any_element()
    }

    fn render_overview_center(
        &self,
        color: gpui::Hsla,
        message: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = cx;
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .px(px(16.0))
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(color)
                    .child(SharedString::from(message)),
            )
            .into_any_element()
    }

    /// The Apps search box: a Waku-style one-line field with a leading search
    /// icon, accent border while focused, on a sunken inset background. A
    /// clear (×) affordance appears once there is something to clear, and a
    /// refresh button re-probes adb on demand.
    fn render_apps_search(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let focused = self.apps_search.read(cx).is_visually_focused(window);
        let has_content = !self.apps_search.read(cx).is_empty();

        let shell = div()
            .id("apps-search")
            .h(px(28.0))
            .flex_1()
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
            .child(div().min_w_0().flex_1().child(self.apps_search.clone()))
            .when(has_content, |element| {
                element.child(
                    div()
                        .id("apps-search-clear")
                        .tab_index(0)
                        .focus_visible(|style| style.bg(theme.overlay))
                        .size(px(18.0))
                        .flex_none()
                        .rounded(px(5.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_default()
                        .hover(|element| element.bg(theme.overlay))
                        .active(|element| element.bg(theme.overlay_strong))
                        .child(icon("icons/x.svg", 11.0, theme.text_ghost))
                        .on_click(cx.listener(|this, _, window, cx| {
                            let field = this.apps_search.clone();
                            field.update(cx, |field, cx| {
                                field.set_content("", cx);
                                field.select_range(0..0, cx);
                            });
                            window.focus(&field.read(cx).focus(), cx);
                        }))
                        .on_key_down(cx.listener(
                            |this, event: &KeyDownEvent, window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    let field = this.apps_search.clone();
                                    field.update(cx, |field, cx| {
                                        field.set_content("", cx);
                                        field.select_range(0..0, cx);
                                    });
                                    window.focus(&field.read(cx).focus(), cx);
                                    cx.stop_propagation();
                                }
                            },
                        )),
                )
            });

        let search_row = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(shell)
            .child(self.render_apps_more_trigger(&theme, cx));

        if self.apps_more_menu_open
            && let Some(bounds) = self.apps_more_menu_bounds.get()
        {
            return search_row
                .child(self.render_apps_more_menu(bounds, cx))
                .into_any_element();
        }
        search_row.into_any_element()
    }

    /// The `⋮` button at the end of the search row that opens the options
    /// menu.
    fn render_apps_more_trigger(&self, theme: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let bounds_ref = self.apps_more_menu_bounds.clone();
        div()
            .id("apps-more-trigger")
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
                this.apps_more_menu_open = !this.apps_more_menu_open;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.apps_more_menu_open = !this.apps_more_menu_open;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
    }

    /// The options dropdown anchored below the `⋮` trigger.
    fn render_apps_more_menu(&self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        self.render_dropdown_card(
            cx,
            "apps-more-menu",
            bounds,
            self.apps_more_menu_bounds.clone(),
            close_apps_more_menu,
            200.0,
            |theme, cx| {
                let mut card = div().flex().flex_col().gap(px(1.0));
                card = card.child(apps_filter_toggle_row(theme, self.apps_more_filter_open, cx));
                if self.apps_more_filter_open {
                    for filter in AppFilter::ALL {
                        card = card.child(apps_filter_option(
                            theme,
                            filter,
                            self.apps_filter == filter,
                            cx,
                        ));
                    }
                }
                card
            },
        )
    }

    /// Apply a new apps filter, persist it, close the options menu, and
    /// re-fetch packages for the selected device.
    pub(crate) fn set_apps_filter(&mut self, filter: AppFilter, cx: &mut Context<Self>) {
        self.apps_filter = filter;
        self.save_settings();
        self.apps_more_filter_open = false;
        self.apps_more_menu_open = false;
        self.refresh_packages(true, cx);
        cx.notify();
    }

    fn render_apps_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let packages = self.filtered_packages(cx);

        if packages.is_empty() {
            return self.render_apps_empty_state(cx);
        }

        let pinned: Vec<&SharedString> = packages
            .iter()
            .filter(|package| self.pinned_apps.contains(package))
            .collect();
        let unpinned: Vec<&SharedString> = packages
            .iter()
            .filter(|package| !self.pinned_apps.contains(package))
            .collect();

        let mut rows = div().flex().flex_col().gap(px(2.0));
        if !pinned.is_empty() {
            rows = rows.child(section_header(&theme, &tr!("apps.list.pinned")));
            for package in &pinned {
                rows = rows.child(self.render_package_row((*package).clone(), cx));
            }
            rows = rows.child(div().h(px(6.0)));
        }
        if !unpinned.is_empty() {
            if !pinned.is_empty() {
                rows = rows.child(section_header(&theme, &self.apps_filter.label()));
            }
            for package in &unpinned {
                rows = rows.child(self.render_package_row((*package).clone(), cx));
            }
        }

        div()
            .id("apps-list-scroll")
            .size_full()
            .overflow_y_scroll()
            .px(px(2.0))
            .child(rows)
            .into_any_element()
    }

    /// One package row: select on left-click, context menu on right-click, and
    /// a pin indicator when pinned.
    fn render_package_row(&self, package: SharedString, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let selected = self.selected_package.as_deref() == Some(package.as_str());
        let pinned = self.pinned_apps.contains(&package);
        div()
            .id(SharedString::from(format!("apps-package-{}", package)))
            .h(px(28.0))
            .flex_none()
            .px(px(8.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .when(selected, |element| element.bg(theme.sidebar_item_background))
            .hover(|element| element.bg(theme.overlay))
            .on_click(cx.listener({
                let package = package.clone();
                move |this, _, _, cx| {
                    this.select_package(package.clone(), cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let package = package.clone();
                    move |this, event: &gpui::MouseDownEvent, _, cx| {
                        this.select_package(package.clone(), cx);
                        this.open_package_context_menu(package.clone(), event.position, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .child(self.render_package_icon(&package, selected, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.5))
                    .text_color(if selected {
                        theme.text
                    } else {
                        theme.text_secondary
                    })
                    .child(package.clone()),
            )
            .when(pinned, |element| {
                element.child(icon("icons/pin.svg", 10.0, theme.text_ghost))
            })
            .into_any_element()
    }

    /// The row's leading glyph: the cached app icon when available, otherwise
    /// the generic apps glyph.
    fn render_package_icon(
        &self,
        package: &SharedString,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let path = self
            .selected_device
            .as_ref()
            .and_then(|serial| self.app_icons.get(serial.as_str()))
            .and_then(|icons| icons.get(package.as_str()))
            .cloned();
        let Some(path) = path else {
            return div()
                .size(px(16.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(icon(
                    "icons/apps.svg",
                    12.0,
                    if selected {
                        theme.text_secondary
                    } else {
                        theme.text_ghost
                    },
                ))
                .into_any_element();
        };
        let fallback_color = if selected {
            theme.text_secondary
        } else {
            theme.text_ghost
        };
        div()
            .size(px(16.0))
            .flex_none()
            .rounded(px(4.0))
            .overflow_hidden()
            .child(
                img(path)
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .with_fallback(move || {
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("icons/apps.svg", 12.0, fallback_color))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    fn render_apps_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let has_query = !self.apps_search.read(cx).is_empty();
        let message = if self.selected_device.is_none() {
            tr!("common.no_device_selected")
        } else if self.packages_loading {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(
                    icon("icons/loader-circle.svg", 14.0, theme.text_tertiary).with_animation(
                        SharedString::from("apps-loading-spinner"),
                        Animation::new(Duration::from_millis(900))
                            .repeat()
                            .with_easing(gpui::linear),
                        |icon, delta| {
                            icon.with_transformation(gpui::Transformation::rotate(
                                gpui::percentage(delta),
                            ))
                        },
                    ),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.text_tertiary)
                        .child(tr_cow!("apps.loading")),
                )
                .into_any_element();
        } else if let Some(error) = &self.packages_error {
            error.clone()
        } else if has_query {
            tr!("apps.no_matching")
        } else {
            tr!("apps.none")
        };
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
}

/// A small uppercase section heading used to separate pinned apps from the
/// rest of the list.
fn section_header(theme: &Theme, label: &str) -> AnyElement {
    div()
        .px(px(8.0))
        .pt(px(6.0))
        .pb(px(2.0))
        .text_size(px(10.0))
        .text_color(theme.text_tertiary)
        .child(SharedString::from(label))
        .into_any_element()
}

/// Close the Apps `⋮` options menu.
fn close_apps_more_menu(this: &mut Hakata, cx: &mut Context<Hakata>) {
    this.apps_more_menu_open = false;
    cx.notify();
}

/// The "Filter by" expandable row in the Apps `⋮` options menu.
fn apps_filter_toggle_row(theme: &Theme, open: bool, cx: &mut Context<Hakata>) -> Stateful<Div> {
    div()
        .id("apps-more-filter")
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
        .on_click(cx.listener(|this, _, _, cx| {
            this.apps_more_filter_open = !this.apps_more_filter_open;
            cx.notify();
        }))
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                this.apps_more_filter_open = !this.apps_more_filter_open;
                cx.notify();
                cx.stop_propagation();
            }
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(11.5))
                .text_color(theme.text_secondary)
                .child(tr_cow!("apps.filter_by")),
        )
        .child(icon("icons/chevron-right.svg", 11.0, theme.text_tertiary).when(open, |icon| {
            icon.with_transformation(gpui::Transformation::rotate(gpui::percentage(0.25)))
        }))
}

/// One selectable filter row in the Apps `⋮` options menu.
fn apps_filter_option(
    theme: &Theme,
    filter: AppFilter,
    selected: bool,
    cx: &mut Context<Hakata>,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("apps-filter-{}", filter.slug())))
        .tab_index(0)
        .pl(px(24.0))
        .pr(px(8.0))
        .h(px(26.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_default()
        .focus_visible(|style| style.bg(theme.overlay))
        .hover(|element| element.bg(theme.overlay))
        .active(|element| element.bg(theme.overlay_strong))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_apps_filter(filter, cx);
        }))
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                this.set_apps_filter(filter, cx);
                cx.stop_propagation();
            }
        }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(11.5))
                .text_color(if selected { theme.text } else { theme.text_secondary })
                .child(filter.label()),
        )
        .when(selected, |element| element.child(icon("icons/check.svg", 11.0, theme.accent)))
}
