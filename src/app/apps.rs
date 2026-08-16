use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, SharedString, Styled, Window, div, px, prelude::*,
};

use crate::theme::Theme;

use super::{Hakata, PanelResizeSide, PanelResizeTarget, icon};

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

    /// The packages the search query leaves visible, matched case-insensitively.
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

        let left_panel = div()
            .w(px(panel_width))
            .h_full()
            .flex_none()
            .relative()
            .flex()
            .flex_col()
            .px(px(12.0))
            .py(px(10.0))
            .child(self.render_apps_search(window, cx))
            .child(div().h(px(8.0)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .child(self.render_apps_list(cx)),
            )
            .child(self.render_panel_resize_handle(
                "apps-resize-handle",
                PanelResizeTarget::Apps,
                PanelResizeSide::Right,
                cx,
            ));

        let right_panel = div()
            .flex_1()
            .h_full()
            .min_w_0()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(13.0))
            .text_color(theme.text_ghost)
            .child(SharedString::from("Coming soon"));

        div()
            .id("apps-page")
            .size_full()
            .flex()
            .child(left_panel)
            .child(right_panel)
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
                    .hover(|element| element.bg(theme.overlay))
                    .child(
                        div()
                            .size(px(14.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("icons/apps.svg", 12.0, theme.text_ghost)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(theme.text_secondary)
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
