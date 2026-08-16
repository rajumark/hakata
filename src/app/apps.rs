use std::time::Duration;

use gpui::{
    actions, Animation, AnimationExt, AnyElement, App, ClipboardItem, Context,
    InteractiveElement, IntoElement, KeyBinding, KeyDownEvent, MouseButton, ParentElement,
    SharedString, Styled, Window, div, px, prelude::*,
};

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
    Files,
}

impl AppsTab {
    pub(crate) const ALL: [Self; 3] = [Self::Overview, Self::Permissions, Self::Files];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Permissions => "Permissions",
            Self::Files => "Files",
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
            cx.notify();
            return;
        };
        if !force
            && self.packages_device.as_deref() == Some(serial.as_str())
            && self.packages_loaded
        {
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
                        .arg("-3")
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
            });
        })
        .detach();
    }

    /// Fetch `adb -s <device> shell dumpsys package <pkg>` for the selected
    /// package and stash the raw output on the entity. The dump is kept until
    /// a different package (or device) is selected; parsing happens on demand
    /// per tab. A generation counter drops a superseded result.
    pub(crate) fn fetch_package_dump(&mut self, cx: &mut Context<Self>) {
        let Some(serial) = self.selected_device.clone() else {
            self.package_dump_raw = None;
            self.package_dump_loading = false;
            self.package_dump_error = None;
            cx.notify();
            return;
        };
        let Some(package) = self.selected_package.clone() else {
            self.package_dump_raw = None;
            self.package_dump_loading = false;
            self.package_dump_error = None;
            cx.notify();
            return;
        };
        if self.package_dump_device.as_deref() == Some(serial.as_str())
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
                        this.package_dump_raw = Some(SharedString::from(
                            String::from_utf8_lossy(&output.stdout),
                        ));
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
                        SharedString::from("No app selected")
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
                    .child(self.render_apps_tab_content(cx)),
            );

        div()
            .id("apps-page")
            .size_full()
            .flex()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .child(left_panel)
            .when(has_selection, |element| element.child(right_panel))
            .into_any_element()
    }

    /// The body for the selected Apps detail tab. Overview shows the parsed
    /// `dumpsys` facts; the other tabs are placeholders for now.
    fn render_apps_tab_content(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.selected_apps_tab {
            AppsTab::Overview => self.render_overview_tab(cx),
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
                    .child(SharedString::from("coming soon")),
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
                        .child(SharedString::from("Loading package info…")),
                )
                .into_any_element();
        }
        if let Some(error) = &self.package_dump_error {
            return self.render_overview_center(theme.text_ghost, error.clone(), cx);
        }
        let Some(info) = self.parsed_package_info() else {
            return self.render_overview_center(theme.text_ghost, "No package info".into(), cx);
        };

        let fields: Vec<(&'static str, String)> = vec![
            ("Version name", info.version_name.unwrap_or_default()),
            ("Version code", info.version_code.unwrap_or_default()),
            ("Target SDK", info.target_sdk.unwrap_or_default()),
            ("Min SDK", info.min_sdk.unwrap_or_default()),
            ("UID", info.uid.unwrap_or_default()),
            (
                "First install",
                info.first_install_time.unwrap_or_default(),
            ),
            ("Last update", info.last_update_time.unwrap_or_default()),
            ("Data dir", info.data_dir.unwrap_or_default()),
            ("Code path", info.code_path.unwrap_or_default()),
            (
                "Flags",
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

        let refresh = div()
            .id("apps-refresh")
            .tab_index(0)
            .focus_visible(|style| style.bg(theme.overlay))
            .size(px(18.0))
            .flex_none()
            .rounded(px(5.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .opacity(if self.packages_loading { 0.6 } else { 1.0 })
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon("icons/refresh-cw.svg", 12.0, theme.text_ghost))
            .on_click(cx.listener(|this, _, _, cx| {
                this.refresh_packages(true, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.refresh_packages(true, cx);
                    cx.stop_propagation();
                }
            }));

        let mut shell = div()
            .id("apps-search")
            .h(px(28.0))
            .flex_none()
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
            .child(div().min_w_0().flex_1().child(self.apps_search.clone()));

        if has_content {
            shell = shell.child(
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
                    .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            let field = this.apps_search.clone();
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

        shell = shell.child(refresh);
        shell.into_any_element()
    }

    fn render_apps_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let packages = self.filtered_packages(cx);

        if packages.is_empty() {
            return self.render_apps_empty_state(cx);
        }

        let mut rows = div().flex().flex_col().gap(px(2.0));
        for package in &packages {
            let selected = self.selected_package.as_deref() == Some(package.as_str());
            rows = rows.child(
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
                            this.selected_package = Some(package.clone());
                            this.title_selected = false;
                            this.fetch_package_dump(cx);
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .size(px(14.0))
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
                            )),
                    )
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
                    ),
            );
        }

        div()
            .id("apps-list-scroll")
            .size_full()
            .overflow_y_scroll()
            .px(px(2.0))
            .child(rows)
            .into_any_element()
    }

    fn render_apps_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let has_query = !self.apps_search.read(cx).is_empty();
        let message = if self.selected_device.is_none() {
            "No device selected".to_string()
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
                        .child(SharedString::from("Loading apps…")),
                )
                .into_any_element();
        } else if let Some(error) = &self.packages_error {
            error.clone()
        } else if has_query {
            "No matching apps".to_string()
        } else {
            "No apps found".to_string()
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
