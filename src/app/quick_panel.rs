use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{
    AnyElement, AnyView, App, Bounds, Context, Div, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, Pixels, Render, SharedString, Stateful, Styled, Window, canvas,
    div, px, prelude::*,
};

use crate::theme::Theme;

use super::{Hakata, icon};

pub(crate) const QUICK_PANEL_WIDTH: f32 = 42.0;
const CHIP_SIZE: f32 = 26.0;
const CHIP_ICON_SIZE: f32 = 16.0;
const GROUP_GAP: f32 = 8.0;
const TAP_MENU_WIDTH: f32 = 140.0;

/// A single-line hint shown by GPUI's `.tooltip(..)` while hovering a chip.
pub(crate) struct Tooltip {
    label: SharedString,
}

impl Tooltip {
    pub(crate) fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
        }
    }

    pub(crate) fn build(self, _window: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|_| self).into()
    }

    pub(crate) fn text(
        label: impl Into<SharedString>,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        let label = label.into();
        move |window, cx| Tooltip::new(label.clone()).build(window, cx)
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        div().pt(px(4.0)).pl(px(2.0)).child(
            div()
                .px(px(7.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.raised)
                .shadow_lg()
                .text_size(px(11.0))
                .line_height(px(15.0))
                .text_color(theme.text_secondary)
                .child(self.label.clone()),
        )
    }
}

impl Hakata {
    /// The vertical quick-action panel docked at the right edge of the main
    /// screen. Rendered only while a device is connected; every chip fires an
    /// ADB command against the selected device.
    pub(crate) fn render_quick_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(serial) = self.selected_device.clone() else {
            return div().into_any_element();
        };
        div()
            .id("quick-panel")
            .w(px(QUICK_PANEL_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .justify_center()
            .pr(px(8.0))
            .child(
                div()
                    .id("quick-panel-scroll")
                    .h_full()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(GROUP_GAP))
                    .py(px(8.0))
                    .child(chip_group_shell(&theme).children([
                        quick_chip(
                            "quick-panel-back",
                            "icons/arrow-left.svg",
                            "Back",
                            serial.clone(),
                            cx,
                            press_back,
                        ),
                        quick_chip(
                            "quick-panel-home",
                            "icons/home.svg",
                            "Home",
                            serial.clone(),
                            cx,
                            press_home,
                        ),
                        quick_chip(
                            "quick-panel-recent",
                            "icons/layers.svg",
                            "Recent",
                            serial.clone(),
                            cx,
                            press_recent,
                        ),
                    ]))
                    .child(chip_group_shell(&theme).children([
                        quick_chip(
                            "quick-panel-volume-up",
                            "icons/volume-2.svg",
                            "Volume Up",
                            serial.clone(),
                            cx,
                            press_volume_up,
                        ),
                        quick_chip(
                            "quick-panel-volume-down",
                            "icons/volume-1.svg",
                            "Volume Down",
                            serial.clone(),
                            cx,
                            press_volume_down,
                        ),
                        quick_chip(
                            "quick-panel-play",
                            "icons/play.svg",
                            "Play",
                            serial.clone(),
                            cx,
                            media_play,
                        ),
                        quick_chip(
                            "quick-panel-pause",
                            "icons/pause.svg",
                            "Pause",
                            serial.clone(),
                            cx,
                            media_pause,
                        ),
                        quick_chip(
                            "quick-panel-mute",
                            "icons/volume-x.svg",
                            "Mute",
                            serial.clone(),
                            cx,
                            volume_mute,
                        ),
                    ]))
                    .child(chip_group_shell(&theme).children([
                        quick_chip(
                            "quick-panel-settings",
                            "icons/settings.svg",
                            "Settings",
                            serial.clone(),
                            cx,
                            open_settings,
                        ),
                        quick_chip(
                            "quick-panel-lock",
                            "icons/lock.svg",
                            "Lock",
                            serial.clone(),
                            cx,
                            press_power,
                        ),
                        quick_chip(
                            "quick-panel-power",
                            "icons/power.svg",
                            "Power",
                            serial.clone(),
                            cx,
                            long_press_power,
                        ),
                        quick_chip(
                            "quick-panel-screenshot",
                            "icons/camera.svg",
                            "Screenshot",
                            serial.clone(),
                            cx,
                            capture_screenshot,
                        ),
                    ]))
                    .child(chip_group_shell(&theme).children([
                        quick_chip(
                            "quick-panel-quick-settings",
                            "icons/layout-grid.svg",
                            "Quick Settings",
                            serial.clone(),
                            cx,
                            expand_quick_settings,
                        ),
                        quick_chip(
                            "quick-panel-notifications",
                            "icons/bell.svg",
                            "Notifications",
                            serial.clone(),
                            cx,
                            expand_notifications,
                        ),
                        quick_chip(
                            "quick-panel-collapse",
                            "icons/chevrons-down-up.svg",
                            "Collapse",
                            serial.clone(),
                            cx,
                            collapse_all,
                        ),
                    ]))
                    .child(
                        chip_group_shell(&theme)
                            .child(quick_chip(
                                "quick-panel-developer",
                                "icons/wrench.svg",
                                "Developer",
                                serial.clone(),
                                cx,
                                open_developer_settings,
                            ))
                            .child(self.render_tap_chip(serial, cx)),
                    ),
            )
            .into_any_element()
    }

    /// The tap-dot chip: clicking it opens a small menu to show or hide the
    /// device's touch indicators (`show_touches`).
    fn render_tap_chip(&self, serial: SharedString, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let bounds_ref = self.tap_menu_bounds.clone();
        let trigger = div()
            .id("quick-panel-tap")
            .relative()
            .tab_index(0)
            .size(px(CHIP_SIZE))
            .flex_none()
            .rounded(px(6.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .focus_visible(|style| style.bg(theme.overlay))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .tooltip(Tooltip::text("Tap options"))
            .child(
                canvas(
                    move |probe: Bounds<Pixels>, _, _| bounds_ref.set(Some(probe)),
                    |_, _, _, _| (),
                )
                .absolute()
                .inset_0(),
            )
            .child(icon("icons/circle-dot.svg", CHIP_ICON_SIZE, theme.text_secondary))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_tap_menu(cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.toggle_tap_menu(cx);
                    cx.stop_propagation();
                }
            }));
        let Some(trigger_bounds) = self.tap_menu_bounds.get() else {
            return trigger.into_any_element();
        };
        if !self.tap_menu_open {
            return trigger.into_any_element();
        }
        let surface = self.render_dropdown_card(
            cx,
            "quick-panel-tap-card",
            trigger_bounds,
            self.tap_menu_bounds.clone(),
            Self::close_tap_menu,
            TAP_MENU_WIDTH,
            move |theme, cx| {
                div()
                    .child(tap_menu_item(
                        "quick-panel-tap-show",
                        "Show tap dot",
                        theme,
                        serial.clone(),
                        cx,
                        Self::quick_show_taps,
                    ))
                    .child(tap_menu_item(
                        "quick-panel-tap-hide",
                        "Hide tap dot",
                        theme,
                        serial.clone(),
                        cx,
                        Self::quick_hide_taps,
                    ))
            },
        );
        trigger.child(surface).into_any_element()
    }

    pub(crate) fn toggle_tap_menu(&mut self, cx: &mut Context<Self>) {
        self.tap_menu_open = !self.tap_menu_open;
        cx.notify();
    }

    pub(crate) fn close_tap_menu(&mut self, cx: &mut Context<Self>) {
        if self.tap_menu_open {
            self.tap_menu_open = false;
            cx.notify();
        }
    }

    pub(crate) fn quick_show_taps(&mut self, serial: SharedString, cx: &mut Context<Self>) {
        self.tap_menu_open = false;
        quick_shell(
            &serial,
            vec!["settings", "put", "system", "show_touches", "1"],
            cx,
        );
    }

    pub(crate) fn quick_hide_taps(&mut self, serial: SharedString, cx: &mut Context<Self>) {
        self.tap_menu_open = false;
        quick_shell(
            &serial,
            vec!["settings", "put", "system", "show_touches", "0"],
            cx,
        );
    }
}

/// The translucent rounded container wrapping a group of chips.
fn chip_group_shell(theme: &Theme) -> Div {
    div()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(2.0))
        .px(px(4.0))
        .py(px(4.0))
        .rounded(px(10.0))
        .bg(theme.raised.opacity(0.6))
}

/// A single 26px icon button: tooltip, hover/focus highlight, click and
/// enter/space activation.
fn quick_chip(
    id: &'static str,
    icon_path: &'static str,
    tooltip: &'static str,
    serial: SharedString,
    cx: &mut Context<Hakata>,
    run: fn(&mut Hakata, SharedString, &mut Context<Hakata>),
) -> Stateful<Div> {
    let theme = Theme::current(cx);
    div()
        .id(id)
        .tab_index(0)
        .size(px(CHIP_SIZE))
        .flex_none()
        .rounded(px(6.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor_default()
        .focus_visible(|style| style.bg(theme.overlay))
        .hover(|element| element.bg(theme.overlay))
        .active(|element| element.bg(theme.overlay_strong))
        .tooltip(Tooltip::text(tooltip))
        .child(icon(icon_path, CHIP_ICON_SIZE, theme.text_secondary))
        .on_click({
            let serial = serial.clone();
            cx.listener(move |this, _, _, cx| {
                run(this, serial.clone(), cx);
            })
        })
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                run(this, serial.clone(), cx);
                cx.stop_propagation();
            }
        }))
}

fn tap_menu_item(
    id: &'static str,
    label: &'static str,
    theme: &Theme,
    serial: SharedString,
    cx: &mut Context<Hakata>,
    run: fn(&mut Hakata, SharedString, &mut Context<Hakata>),
) -> Stateful<Div> {
    div()
        .id(id)
        .mx(px(4.0))
        .px(px(8.0))
        .min_h(px(26.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_default()
        .text_size(px(11.5))
        .text_color(theme.text_secondary)
        .hover(|element| element.bg(theme.overlay))
        .active(|element| element.bg(theme.overlay_strong))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                run(this, serial.clone(), cx);
                cx.stop_propagation();
            }),
        )
        .child(SharedString::from(label))
}

/// Fire `adb -s <serial> shell <args>` on the background executor. One-shot:
/// the outcome is not surfaced in the UI.
fn quick_shell(serial: &SharedString, args: Vec<&'static str>, cx: &mut Context<Hakata>) {
    if !crate::adb::is_installed() {
        return;
    }
    let adb_path = crate::adb::adb_path();
    let serial_for_spawn = serial.to_string();
    let args: Vec<String> = args.into_iter().map(str::to_string).collect();
    cx.spawn(async move |_, cx| {
        let _ = cx
            .background_executor()
            .spawn(async move {
                let _ = std::process::Command::new(&adb_path)
                    .arg("-s")
                    .arg(&serial_for_spawn)
                    .arg("shell")
                    .args(&args)
                    .output();
            })
            .await;
    })
    .detach();
}

fn press_back(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["input", "keyevent", "4"], cx);
}

fn press_home(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["input", "keyevent", "3"], cx);
}

fn press_recent(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["input", "keyevent", "187"], cx);
}

fn press_volume_up(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["input", "keyevent", "24"], cx);
}

fn press_volume_down(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["input", "keyevent", "25"], cx);
}

fn media_play(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["input", "keyevent", "126"], cx);
}

fn media_pause(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["input", "keyevent", "127"], cx);
}

fn volume_mute(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["input", "keyevent", "164"], cx);
}

fn open_settings(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["am", "start", "-a", "android.settings.SETTINGS"], cx);
}

fn press_power(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["input", "keyevent", "26"], cx);
}

fn long_press_power(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(
        &serial,
        vec!["input", "keyevent", "--longpress", "26"],
        cx,
    );
}

fn expand_quick_settings(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["cmd", "statusbar", "expand-settings"], cx);
}

fn expand_notifications(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["cmd", "statusbar", "expand-notifications"], cx);
}

fn collapse_all(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(&serial, vec!["cmd", "statusbar", "collapse"], cx);
}

fn open_developer_settings(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    quick_shell(
        &serial,
        vec![
            "am",
            "start",
            "-a",
            "android.settings.APPLICATION_DEVELOPMENT_SETTINGS",
        ],
        cx,
    );
}

/// Capture a screenshot, pull it into the host Downloads folder, and reveal
/// that folder in the platform file manager.
fn capture_screenshot(_: &mut Hakata, serial: SharedString, cx: &mut Context<Hakata>) {
    if !crate::adb::is_installed() {
        return;
    }
    let adb_path = crate::adb::adb_path();
    let serial_for_spawn = serial.to_string();
    cx.spawn(async move |_, cx| {
        let saved = cx
            .background_executor()
            .spawn(async move {
                let screencap = std::process::Command::new(&adb_path)
                    .arg("-s")
                    .arg(&serial_for_spawn)
                    .arg("shell")
                    .arg("screencap")
                    .arg("-p")
                    .arg("/sdcard/screenshot.png")
                    .output();
                if !matches!(screencap, Ok(output) if output.status.success()) {
                    return None;
                }
                let downloads = dirs::download_dir()?;
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_millis())
                    .unwrap_or(0);
                let file_path = downloads.join(format!("screenshot_{}.png", timestamp));
                let pull = std::process::Command::new(&adb_path)
                    .arg("-s")
                    .arg(&serial_for_spawn)
                    .arg("pull")
                    .arg("/sdcard/screenshot.png")
                    .arg(&file_path)
                    .output();
                if matches!(pull, Ok(output) if output.status.success()) {
                    Some(downloads)
                } else {
                    None
                }
            })
            .await;
        if let Some(downloads) = saved {
            reveal_in_file_manager(&downloads);
        }
    })
    .detach();
}

fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
}
