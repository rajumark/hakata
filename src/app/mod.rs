use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Anchor, AnyElement, App, Bounds, Context, Div, Entity, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Pixels, Point, Render, SharedString, Stateful, Styled, Subscription, Svg, Window,
    anchored, canvas, deferred, div, prelude::*, px,
};

use crate::input::{SearchField, SearchFieldEvent};
use crate::theme::{Theme, ThemePreference};

pub mod apps;
pub mod debug;
pub mod package_info;
pub mod settings;
pub mod sidebar;

#[cfg(target_os = "macos")]
pub(crate) const TRAFFIC_LIGHT_CLEARANCE: f32 = 86.0;
#[cfg(not(target_os = "macos"))]
pub(crate) const TRAFFIC_LIGHT_CLEARANCE: f32 = 8.0;

pub(crate) const SIDEBAR_MIN_WIDTH: f32 = 180.0;
pub(crate) const SIDEBAR_MAX_WIDTH: f32 = 420.0;
pub(crate) const SIDEBAR_DEFAULT_WIDTH: f32 = 252.0;
pub(crate) const MAIN_PANEL_MIN_WIDTH: f32 = 360.0;
pub(crate) const APPS_PANEL_MIN_WIDTH: f32 = 200.0;
pub(crate) const APPS_PANEL_FRACTION: f32 = 0.30;
pub(crate) const APPS_PANEL_MAX_FRACTION: f32 = 0.60;

/// How often the connected-device list is refreshed.
pub(crate) const DEVICE_REFRESH_INTERVAL: Duration = Duration::from_secs(3);

/// The four sidebar menus. Each shows a page in the main area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MenuPage {
    NewTask,
    Search,
    Apps,
    Settings,
    Debug,
}

impl MenuPage {
    pub(crate) const ALL: [Self; 5] = [
        Self::NewTask,
        Self::Search,
        Self::Apps,
        Self::Settings,
        Self::Debug,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::NewTask => "New Task",
            Self::Search => "Search",
            Self::Apps => "Apps",
            Self::Settings => "Settings",
            Self::Debug => "Debug",
        }
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::NewTask => "icons/compose.svg",
            Self::Search => "icons/search.svg",
            Self::Apps => "icons/apps.svg",
            Self::Settings => "icons/settings.svg",
            Self::Debug => "icons/terminal-square.svg",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanelResizeTarget {
    Sidebar,
    Apps,
}

/// Which edge of its parent pane a resize strip straddles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PanelResizeSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PanelResizeDrag {
    target: PanelResizeTarget,
    start_mouse_x: f32,
    start_width: f32,
}

pub struct Hakata {
    pub(crate) selected_page: MenuPage,
    pub(crate) sidebar_visible: bool,
    pub(crate) sidebar_width: f32,
    pub(crate) panel_resize_drag: Option<PanelResizeDrag>,
    pub(crate) header_drag_armed: bool,
    pub(crate) toggle_focus: FocusHandle,
    pub(crate) adb_version: Option<debug::AdbVersionStatus>,
    pub(crate) adb_bootstrap: Option<debug::AdbBootstrapState>,
    pub(crate) device_refresh_started: bool,
    pub(crate) devices: Vec<crate::adb::AdbDevice>,
    pub(crate) selected_device: Option<SharedString>,
    pub(crate) device_menu_open: bool,
    pub(crate) device_trigger_focus: FocusHandle,
    pub(crate) device_trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) theme_preference: ThemePreference,
    pub(crate) theme_menu_open: bool,
    pub(crate) theme_trigger_focus: FocusHandle,
    pub(crate) theme_trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) appearance_observed: bool,
    pub(crate) apps_search: Entity<SearchField>,
    pub(crate) _apps_search_subscription: Subscription,
    pub(crate) packages: Vec<SharedString>,
    pub(crate) packages_loading: bool,
    pub(crate) packages_loaded: bool,
    pub(crate) packages_device: Option<SharedString>,
    pub(crate) packages_error: Option<String>,
    pub(crate) packages_refresh_epoch: usize,
    pub(crate) selected_package: Option<SharedString>,
    pub(crate) selected_apps_tab: apps::AppsTab,
    pub(crate) apps_title_focus: FocusHandle,
    pub(crate) title_selected: bool,
    pub(crate) package_dump_device: Option<SharedString>,
    pub(crate) package_dump_package: Option<SharedString>,
    pub(crate) package_dump_raw: Option<SharedString>,
    pub(crate) package_dump_loading: bool,
    pub(crate) package_dump_error: Option<String>,
    pub(crate) package_dump_epoch: usize,
    pub(crate) apps_panel_width: f32,
    pub(crate) emulator_dialog_open: bool,
    pub(crate) emulators: Vec<String>,
    pub(crate) emulators_loaded: bool,
    pub(crate) emulators_loading: bool,
    pub(crate) emulators_error: Option<String>,
    pub(crate) emulators_refresh_epoch: usize,
    pub(crate) emulator_launching: Option<String>,
    pub(crate) emulator_start_error: Option<String>,
}

impl Hakata {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let apps_search = cx.new(|cx| SearchField::new(cx).placeholder("Search apps"));
            let _apps_search_subscription =
                cx.subscribe(&apps_search, |_, _, _: &SearchFieldEvent, cx| {
                    cx.notify();
                });
            Self {
                selected_page: MenuPage::NewTask,
                sidebar_visible: true,
                sidebar_width: SIDEBAR_DEFAULT_WIDTH,
                panel_resize_drag: None,
                header_drag_armed: false,
                toggle_focus: cx.focus_handle(),
                adb_version: None,
                adb_bootstrap: None,
                device_refresh_started: false,
                devices: Vec::new(),
                selected_device: None,
                device_menu_open: false,
                device_trigger_focus: cx.focus_handle(),
                device_trigger_bounds: Rc::new(Cell::new(None)),
                theme_preference: crate::theme::theme_preference(cx),
                theme_menu_open: false,
                theme_trigger_focus: cx.focus_handle(),
                theme_trigger_bounds: Rc::new(Cell::new(None)),
                appearance_observed: false,
                apps_search,
                _apps_search_subscription,
                packages: Vec::new(),
                packages_loading: false,
                packages_loaded: false,
                packages_device: None,
                packages_error: None,
                packages_refresh_epoch: 0,
                selected_package: None,
                selected_apps_tab: apps::AppsTab::Overview,
                apps_title_focus: cx.focus_handle(),
                title_selected: false,
                package_dump_device: None,
                package_dump_package: None,
                package_dump_raw: None,
                package_dump_loading: false,
                package_dump_error: None,
                package_dump_epoch: 0,
                apps_panel_width: 0.0,
                emulator_dialog_open: false,
                emulators: Vec::new(),
                emulators_loaded: false,
                emulators_loading: false,
                emulators_error: None,
                emulators_refresh_epoch: 0,
                emulator_launching: None,
                emulator_start_error: None,
            }
        })
    }

    fn effective_sidebar_width(&self, window: &Window) -> f32 {
        if !self.sidebar_visible {
            return 0.0;
        }
        let viewport_width = f32::from(window.viewport_size().width);
        self.sidebar_width.clamp(
            SIDEBAR_MIN_WIDTH,
            SIDEBAR_MAX_WIDTH
                .min(viewport_width - MAIN_PANEL_MIN_WIDTH)
                .max(SIDEBAR_MIN_WIDTH),
        )
    }

    pub(crate) fn effective_apps_panel_width(&self, window: &Window) -> f32 {
        let viewport_width = f32::from(window.viewport_size().width);
        let default = viewport_width * APPS_PANEL_FRACTION;
        let base = if self.apps_panel_width > 0.0 {
            self.apps_panel_width
        } else {
            default
        };
        base.clamp(APPS_PANEL_MIN_WIDTH, viewport_width * APPS_PANEL_MAX_FRACTION)
    }

    pub(crate) fn select_page(&mut self, page: MenuPage, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_page = page;
        if page == MenuPage::Debug {
            self.check_adb_version(cx);
        }
        if page == MenuPage::Apps {
            self.refresh_packages(false, cx);
            self.fetch_package_dump(cx);
            window.focus(&self.apps_search.read(cx).focus(), cx);
        }
        cx.notify();
    }

    pub(crate) fn set_sidebar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.sidebar_visible == visible {
            return;
        }
        self.sidebar_visible = visible;
        cx.notify();
    }

    pub(crate) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.set_sidebar_visible(!self.sidebar_visible, cx);
    }

    // ── Window drag ───────────────────────────────────────────────────────

    pub(crate) fn window_drag_region(
        &self,
        region: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        region
            .on_click(|event, window, _| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            })
            .on_mouse_down_out(cx.listener(|this, _, _, _| {
                this.header_drag_armed = false;
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.header_drag_armed = true;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.header_drag_armed = false;
                }),
            )
            .on_mouse_move(cx.listener(|this, _, window, _| {
                if this.header_drag_armed {
                    this.header_drag_armed = false;
                    window.start_window_move();
                }
            }))
    }

    // ── Layout ────────────────────────────────────────────────────────────

    /// The main content area's 48px header. When the sidebar is hidden it
    /// hosts the traffic-light clearance, sidebar toggle, and history buttons
    /// (exactly like Waku), and its drag regions move the whole window.
    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::current(cx);
        let page = self.selected_page;
        div()
            .id("window-header")
            .h(px(48.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(if self.sidebar_visible {
                px(14.0)
            } else {
                px(0.0)
            })
            .pr(px(14.0))
            .when(!self.sidebar_visible, |element| {
                element
                    .child(
                        self.window_drag_region(
                            div()
                                .id("header-traffic-light-drag-region")
                                .w(px(TRAFFIC_LIGHT_CLEARANCE - 8.0))
                                .h_full()
                                .flex_none(),
                            cx,
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(self.sidebar_toggle(cx))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(2.0))
                                    .child(self.history_button(
                                        "navigate-back",
                                        "icons/arrow-left.svg",
                                        false,
                                        cx,
                                    ))
                                    .child(self.history_button(
                                        "navigate-forward",
                                        "icons/arrow-right.svg",
                                        false,
                                        cx,
                                    )),
                            ),
                    )
            })
            .child(
                self.window_drag_region(
                    div()
                        .id("header-title-drag-region")
                        .h_full()
                        .min_w_0()
                        .flex_shrink(1.0)
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(13.0))
                                .text_color(theme.text)
                                .child(page.label()),
                        ),
                    cx,
                ),
            )
            .child(
                self.window_drag_region(
                    div().id("header-center-drag-region").h_full().flex_1(),
                    cx,
                ),
            )
    }

    // ── Panel resize ──────────────────────────────────────────────────────

    /// The draggable divider between two panes. `side` says which edge of the
    /// parent pane the strip straddles: the sidebar divider hangs off the
    /// content column's left edge, the Apps divider off the list panel's right
    /// edge.
    pub(crate) fn render_panel_resize_handle(
        &self,
        id: &'static str,
        target: PanelResizeTarget,
        side: PanelResizeSide,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let active = self.panel_resize_drag.is_some_and(|drag| drag.target == target);
        let line = match side {
            PanelResizeSide::Left => div()
                .absolute()
                .top_0()
                .h_full()
                .w(px(2.0))
                .rounded(px(1.0))
                .left(px(5.0))
                .bg(if active {
                    theme.resize_handle
                } else {
                    gpui::transparent_black()
                })
                .group_hover("panel-resize-handle", |element| {
                    element.bg(theme.resize_handle)
                }),
            PanelResizeSide::Right => div()
                .absolute()
                .top_0()
                .h_full()
                .w(px(2.0))
                .rounded(px(1.0))
                .right(px(5.0))
                .bg(if active {
                    theme.resize_handle
                } else {
                    gpui::transparent_black()
                })
                .group_hover("panel-resize-handle", |element| {
                    element.bg(theme.resize_handle)
                }),
        };
        div()
            .id(id)
            .absolute()
            .top_0()
            .w(px(10.0))
            .h_full()
            .group("panel-resize-handle")
            .cursor_col_resize()
            .when(side == PanelResizeSide::Left, |element| {
                element.left(px(-5.0))
            })
            .when(side == PanelResizeSide::Right, |element| {
                element.right(px(-5.0))
            })
            .child(line)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event, window, cx| {
                    this.begin_panel_resize(target, event, window, cx);
                }),
            )
    }

    fn begin_panel_resize(
        &mut self,
        target: PanelResizeTarget,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start_width = match target {
            PanelResizeTarget::Sidebar => self.effective_sidebar_width(window),
            PanelResizeTarget::Apps => {
                let start = self.effective_apps_panel_width(window);
                self.apps_panel_width = start;
                start
            }
        };
        self.panel_resize_drag = Some(PanelResizeDrag {
            target,
            start_mouse_x: f32::from(event.position.x),
            start_width,
        });
        cx.stop_propagation();
        cx.notify();
    }

    fn resize_panel_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.panel_resize_drag else {
            return;
        };
        let viewport_width = f32::from(window.viewport_size().width);
        let delta = f32::from(event.position.x) - drag.start_mouse_x;
        let width = match drag.target {
            PanelResizeTarget::Sidebar => {
                let maximum = SIDEBAR_MAX_WIDTH
                    .min(viewport_width - MAIN_PANEL_MIN_WIDTH)
                    .max(SIDEBAR_MIN_WIDTH);
                (drag.start_width + delta).clamp(SIDEBAR_MIN_WIDTH, maximum)
            }
            PanelResizeTarget::Apps => {
                (drag.start_width + delta)
                    .clamp(APPS_PANEL_MIN_WIDTH, viewport_width * APPS_PANEL_MAX_FRACTION)
            }
        };
        let field = match drag.target {
            PanelResizeTarget::Sidebar => &mut self.sidebar_width,
            PanelResizeTarget::Apps => &mut self.apps_panel_width,
        };
        if (*field - width).abs() < 0.5 {
            return;
        }
        *field = width;
        cx.notify();
    }

    fn finish_panel_resize(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.button == MouseButton::Left && self.panel_resize_drag.take().is_some() {
            cx.notify();
        }
    }

    // ── Connected devices ────────────────────────────────────────────────

    /// Keep the device list fresh: `adb devices` on the background executor
    /// every [`DEVICE_REFRESH_INTERVAL`]. Never touches the UI thread.
    fn start_device_refresh(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                let adb_path = crate::adb::adb_path();
                let output = crate::adb::is_installed().then(|| {
                    cx.background_executor().spawn(async move {
                        std::process::Command::new(&adb_path)
                            .arg("devices")
                            .output()
                    })
                });
                let devices = match output {
                    Some(task) => task
                        .await
                        .ok()
                        .filter(|output| output.status.success())
                        .map(|output| {
                            crate::adb::parse_adb_devices(&String::from_utf8_lossy(&output.stdout))
                        })
                        .unwrap_or_default(),
                    None => Vec::new(),
                };
                if this
                    .update(cx, |this, cx| this.set_devices(devices, cx))
                    .is_err()
                {
                    return;
                }
                cx.background_executor()
                    .timer(DEVICE_REFRESH_INTERVAL)
                    .await;
            }
        })
        .detach();
    }

    /// Replace the device list, keeping the current selection while it is
    /// still attached and ready, otherwise auto-selecting the first ready
    /// device so there is always one default when devices exist.
    fn set_devices(&mut self, devices: Vec<crate::adb::AdbDevice>, cx: &mut Context<Self>) {
        let ready: Vec<&str> = devices
            .iter()
            .filter(|device| device.state == "device")
            .map(|device| device.serial.as_str())
            .collect();
        let next = crate::adb::resolve_default_device(self.selected_device.as_deref(), &ready);
        self.selected_device = next.map(SharedString::from);
        self.devices = devices;
        if self.selected_page == MenuPage::Apps {
            self.refresh_packages(false, cx);
            self.fetch_package_dump(cx);
        }
        cx.notify();
    }

    pub(crate) fn toggle_device_menu(&mut self, cx: &mut Context<Self>) {
        self.device_menu_open = !self.device_menu_open;
        cx.notify();
    }

    pub(crate) fn close_device_menu(&mut self, cx: &mut Context<Self>) {
        if self.device_menu_open {
            self.device_menu_open = false;
            cx.notify();
        }
    }

    pub(crate) fn select_device(&mut self, serial: &str, cx: &mut Context<Self>) {
        self.selected_device = Some(SharedString::from(serial));
        self.device_menu_open = false;
        if self.selected_page == MenuPage::Apps {
            self.refresh_packages(false, cx);
            self.fetch_package_dump(cx);
        }
        cx.notify();
    }

    // ── Theme preference ─────────────────────────────────────────────────

    pub(crate) fn toggle_theme_menu(&mut self, cx: &mut Context<Self>) {
        self.theme_menu_open = !self.theme_menu_open;
        cx.notify();
    }

    pub(crate) fn close_theme_menu(&mut self, cx: &mut Context<Self>) {
        if self.theme_menu_open {
            self.theme_menu_open = false;
            cx.notify();
        }
    }

    pub(crate) fn set_theme_preference(
        &mut self,
        preference: ThemePreference,
        cx: &mut Context<Self>,
    ) {
        if self.theme_preference == preference {
            return;
        }
        self.theme_preference = preference;
        crate::theme::set_theme_preference(preference, cx);
        let _ = crate::settings::save(&crate::settings::Settings { theme: preference });
        self.theme_menu_open = false;
        cx.notify();
    }

    // ── Dropdowns ────────────────────────────────────────────────────────

    /// The dropdown chrome shared by every menu on the page: a focusable
    /// trigger that records its bounds and toggles the menu. Returns the
    /// element with its children attached by the caller.
    pub(crate) fn render_trigger(
        &self,
        cx: &mut Context<Self>,
        id: &'static str,
        focus: &FocusHandle,
        bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
        toggle: fn(&mut Self, &mut Context<Self>),
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let bounds_ref = bounds.clone();
        div()
            .id(id)
            .relative()
            .track_focus(focus)
            .tab_index(0)
            .w_full()
            .h(px(30.0))
            .px(px(8.0))
            .rounded(px(7.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.raised)
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .focus_visible(|style| style.border_1().border_color(theme.accent))
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    toggle(this, cx);
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    toggle(this, cx);
                    cx.stop_propagation();
                }
            }))
    }

    /// The dropdown popover: a deferred card anchored just below the trigger,
    /// closed by a mouse-down anywhere outside of it (or on the trigger).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_dropdown_card(
        &self,
        cx: &mut Context<Self>,
        id: &'static str,
        trigger_bounds: Bounds<Pixels>,
        bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
        close: fn(&mut Self, &mut Context<Self>),
        width: f32,
        body: impl Fn(&Theme, &mut Context<Self>) -> Div,
    ) -> AnyElement {
        let theme = Theme::current(cx);
        let card = div()
            .id(id)
            .occlude()
            .w(px(width))
            .py(px(4.0))
            .rounded(px(9.0))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.raised)
            .shadow_lg()
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let on_trigger = bounds
                    .get()
                    .is_some_and(|bounds| bounds.contains(&event.position))
                    && event.button == MouseButton::Left;
                if !on_trigger {
                    close(this, cx);
                }
            }))
            .child(body(&theme, cx));
        deferred(
            anchored()
                .anchor(Anchor::TopLeft)
                .position(Point::new(
                    trigger_bounds.left(),
                    trigger_bounds.bottom() + px(4.0),
                ))
                .child(card),
        )
        .with_priority(4)
        .into_any_element()
    }

    // ── Pages ─────────────────────────────────────────────────────────────

    fn render_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        match self.selected_page {
            MenuPage::Debug => self.render_debug_page(cx),
            MenuPage::Settings => self.render_settings_page(cx),
            MenuPage::Apps => self.render_apps_page(window, cx),
            page => {
                let theme = Theme::current(cx);
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .text_size(px(14.0))
                            .text_color(theme.text_secondary)
                            .child(icon(page.icon(), 16.0, theme.text_ghost))
                            .child(SharedString::from(format!("{} coming soon", page.label()))),
                    )
                    .into_any_element()
            }
        }
    }
}

/// A monochrome icon from the embedded set, tinted via text color.
pub(crate) fn icon(path: &'static str, size: f32, color: gpui::Hsla) -> Svg {
    gpui::svg()
        .path(path)
        .w(px(size))
        .h(px(size))
        .flex_none()
        .text_color(color)
}

impl Render for Hakata {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.adb_bootstrap.is_none() && !crate::adb::is_installed() {
            self.start_adb_bootstrap(cx);
        }
        if !self.device_refresh_started {
            self.device_refresh_started = true;
            self.start_device_refresh(cx);
        }
        if !self.appearance_observed {
            self.appearance_observed = true;
            cx.observe_window_appearance(window, |_, window, _| {
                window.refresh();
            })
            .detach();
        }
        let theme = Theme::current(cx);
        let sidebar_width = self.effective_sidebar_width(window);
        let content = div()
            .key_context("Hakata")
            .capture_any_mouse_down(cx.listener(|this, _, _, _| {
                this.header_drag_armed = false;
            }))
            .on_mouse_move(cx.listener(Self::resize_panel_mouse_move))
            .capture_any_mouse_up(cx.listener(Self::finish_panel_resize))
            .size_full()
            .relative()
            .flex()
            .text_color(theme.text)
            .font_family(".SystemUIFont")
            .when(self.sidebar_visible, |root| {
                root.child(self.render_sidebar(sidebar_width, cx))
            })
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .bg(theme.surface)
                    .when(self.sidebar_visible, |element| {
                        element.border_l_1().border_color(theme.sidebar_border)
                    })
                    .child(self.render_header(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .child(self.render_page(window, cx)),
                    )
                    .when(self.sidebar_visible, |element| {
                        element.child(self.render_panel_resize_handle(
                            "sidebar-resize-handle",
                            PanelResizeTarget::Sidebar,
                            PanelResizeSide::Left,
                            cx,
                        ))
                    }),
            )
            .into_any_element();
        let root = div().size_full().relative().child(content);
        let root = if let Some(modal) = self.render_adb_bootstrap_modal(cx) {
            root.child(modal)
        } else {
            root
        };
        let root = if let Some(modal) = self.render_emulators_dialog(cx) {
            root.child(modal)
        } else {
            root
        };
        root.into_any_element()
    }
}
