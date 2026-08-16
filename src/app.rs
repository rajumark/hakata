use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Anchor, Animation, AnimationExt, AnyElement, App, Bounds, ClipboardItem, Context,
    DefiniteLength, Div, Entity, FocusHandle, FontWeight, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Render, SharedString, Stateful, Styled, Subscription, Svg, Window, anchored, canvas,
    deferred, div, prelude::*, px,
};

use crate::input::{SearchField, SearchFieldEvent};
use crate::theme::{Theme, ThemePreference};

#[cfg(target_os = "macos")]
const TRAFFIC_LIGHT_CLEARANCE: f32 = 86.0;
#[cfg(not(target_os = "macos"))]
const TRAFFIC_LIGHT_CLEARANCE: f32 = 8.0;

const SIDEBAR_MIN_WIDTH: f32 = 180.0;
const SIDEBAR_MAX_WIDTH: f32 = 420.0;
const SIDEBAR_DEFAULT_WIDTH: f32 = 252.0;
const MAIN_PANEL_MIN_WIDTH: f32 = 360.0;
const SIDEBAR_ACTION_ROW_HEIGHT: f32 = 32.0;
const TITLEBAR_HEIGHT: f32 = 48.0;
const FOOTER_HEIGHT: f32 = 40.0;
/// The Apps list panel starts at 30% of the viewport and never goes below a
/// usable width or past 60%.
const APPS_PANEL_MIN_WIDTH: f32 = 200.0;
const APPS_PANEL_FRACTION: f32 = 0.30;
const APPS_PANEL_MAX_FRACTION: f32 = 0.60;

/// The four sidebar menus. Each shows a page in the main area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuPage {
    NewTask,
    Search,
    Apps,
    Settings,
    Debug,
}

impl MenuPage {
    const ALL: [Self; 5] = [
        Self::NewTask,
        Self::Search,
        Self::Apps,
        Self::Settings,
        Self::Debug,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::NewTask => "New Task",
            Self::Search => "Search",
            Self::Apps => "Apps",
            Self::Settings => "Settings",
            Self::Debug => "Debug",
        }
    }

    fn icon(self) -> &'static str {
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
enum PanelResizeTarget {
    Sidebar,
    Apps,
}

/// Which edge of its parent pane a resize strip straddles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PanelResizeSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
struct PanelResizeDrag {
    target: PanelResizeTarget,
    start_mouse_x: f32,
    start_width: f32,
}

/// The adb version probe run when the Debug page is opened.
enum AdbVersionStatus {
    Checking,
    Version(String),
    Error(String),
}

/// The launch-time adb bootstrap: download platform-tools on first run.
enum AdbBootstrapState {
    Downloading { progress: f32 },
    Error(String),
    Done,
}

/// How often the connected-device list is refreshed.
const DEVICE_REFRESH_INTERVAL: Duration = Duration::from_secs(3);

pub struct Hakata {
    selected_page: MenuPage,
    sidebar_visible: bool,
    sidebar_width: f32,
    panel_resize_drag: Option<PanelResizeDrag>,
    header_drag_armed: bool,
    toggle_focus: FocusHandle,
    adb_version: Option<AdbVersionStatus>,
    adb_bootstrap: Option<AdbBootstrapState>,
    device_refresh_started: bool,
    devices: Vec<crate::adb::AdbDevice>,
    selected_device: Option<SharedString>,
    device_menu_open: bool,
    device_trigger_focus: FocusHandle,
    device_trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    theme_preference: ThemePreference,
    theme_menu_open: bool,
    theme_trigger_focus: FocusHandle,
    theme_trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    appearance_observed: bool,
    apps_search: Entity<SearchField>,
    _apps_search_subscription: Subscription,
    packages: Vec<SharedString>,
    packages_loading: bool,
    packages_loaded: bool,
    packages_device: Option<SharedString>,
    packages_error: Option<String>,
    packages_refresh_epoch: usize,
    apps_panel_width: f32,
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
                apps_panel_width: 0.0,
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

    fn select_page(&mut self, page: MenuPage, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_page = page;
        if page == MenuPage::Debug {
            self.check_adb_version(cx);
        }
        if page == MenuPage::Apps {
            self.refresh_packages(false, cx);
            window.focus(&self.apps_search.read(cx).focus(), cx);
        }
        cx.notify();
    }

    fn effective_apps_panel_width(&self, window: &Window) -> f32 {
        let viewport_width = f32::from(window.viewport_size().width);
        let default = viewport_width * APPS_PANEL_FRACTION;
        let base = if self.apps_panel_width > 0.0 {
            self.apps_panel_width
        } else {
            default
        };
        base.clamp(APPS_PANEL_MIN_WIDTH, viewport_width * APPS_PANEL_MAX_FRACTION)
    }

    /// Probe `adb version` on the background executor the first time the Debug
    /// page is shown. Rendering only ever reads the cached status.
    fn check_adb_version(&mut self, cx: &mut Context<Self>) {
        if self.adb_version.is_some() {
            return;
        }
        let adb_path = crate::adb::adb_path();
        if !crate::adb::is_installed() {
            self.adb_version = Some(AdbVersionStatus::Error(format!(
                "adb not found at {}",
                adb_path.display()
            )));
            cx.notify();
            return;
        }
        self.adb_version = Some(AdbVersionStatus::Checking);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    std::process::Command::new(&adb_path)
                        .arg("version")
                        .output()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.adb_version = Some(match result {
                    Ok(output) if output.status.success() => AdbVersionStatus::Version(
                        String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    ),
                    Ok(output) => AdbVersionStatus::Error(
                        String::from_utf8_lossy(&output.stderr).trim().to_string(),
                    ),
                    Err(error) => AdbVersionStatus::Error(error.to_string()),
                });
                cx.notify();
            });
        })
        .detach();
    }

    // ── adb bootstrap ────────────────────────────────────────────────────

    /// Download platform-tools on launch when adb is missing. Runs off the UI
    /// thread; progress streams through a bounded channel into the modal.
    fn start_adb_bootstrap(&mut self, cx: &mut Context<Self>) {
        if self.adb_bootstrap.is_some() || crate::adb::is_installed() {
            return;
        }
        self.adb_bootstrap = Some(AdbBootstrapState::Downloading { progress: 0.0 });
        cx.notify();

        let (progress_sender, progress_receiver) = smol::channel::bounded::<f32>(16);
        cx.spawn(async move |this, cx| {
            let task = cx.background_executor().spawn(async move {
                crate::adb::download_and_install(move |fraction| {
                    let _ = progress_sender.send_blocking(fraction);
                })
            });

            while let Ok(fraction) = progress_receiver.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(AdbBootstrapState::Downloading { progress }) =
                        &mut this.adb_bootstrap
                    {
                        *progress = fraction;
                    }
                    cx.notify();
                });
            }

            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.adb_bootstrap = Some(match result {
                    Ok(()) => {
                        this.adb_version = None;
                        if this.selected_page == MenuPage::Debug {
                            this.check_adb_version(cx);
                        }
                        if this.selected_page == MenuPage::Apps {
                            this.refresh_packages(false, cx);
                        }
                        AdbBootstrapState::Done
                    }
                    Err(error) => AdbBootstrapState::Error(error.to_string()),
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn retry_adb_bootstrap(&mut self, cx: &mut Context<Self>) {
        self.adb_bootstrap = None;
        self.start_adb_bootstrap(cx);
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
        }
        cx.notify();
    }

    fn toggle_device_menu(&mut self, cx: &mut Context<Self>) {
        self.device_menu_open = !self.device_menu_open;
        cx.notify();
    }

    fn close_device_menu(&mut self, cx: &mut Context<Self>) {
        if self.device_menu_open {
            self.device_menu_open = false;
            cx.notify();
        }
    }

    fn select_device(&mut self, serial: &str, cx: &mut Context<Self>) {
        self.selected_device = Some(SharedString::from(serial));
        self.device_menu_open = false;
        if self.selected_page == MenuPage::Apps {
            self.refresh_packages(false, cx);
        }
        cx.notify();
    }

    // ── Apps packages ─────────────────────────────────────────────────────

    /// Fetch third-party packages for the selected device on the background
    /// executor. `force` bypasses the "already fresh for this device" guard so
    /// the refresh button always hits adb. A generation counter drops results
    /// from a superseded run (device switched mid-flight).
    fn refresh_packages(&mut self, force: bool, cx: &mut Context<Self>) {
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

    // ── Theme preference ─────────────────────────────────────────────────

    fn toggle_theme_menu(&mut self, cx: &mut Context<Self>) {
        self.theme_menu_open = !self.theme_menu_open;
        cx.notify();
    }

    fn close_theme_menu(&mut self, cx: &mut Context<Self>) {
        if self.theme_menu_open {
            self.theme_menu_open = false;
            cx.notify();
        }
    }

    fn set_theme_preference(&mut self, preference: ThemePreference, cx: &mut Context<Self>) {
        if self.theme_preference == preference {
            return;
        }
        self.theme_preference = preference;
        crate::theme::set_theme_preference(preference, cx);
        let _ = crate::settings::save(&crate::settings::Settings { theme: preference });
        self.theme_menu_open = false;
        cx.notify();
    }

    pub fn set_sidebar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.sidebar_visible == visible {
            return;
        }
        self.sidebar_visible = visible;
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.set_sidebar_visible(!self.sidebar_visible, cx);
    }

    // ── Window drag ───────────────────────────────────────────────────────

    fn window_drag_region(&self, region: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
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

    // ── Sidebar controls ──────────────────────────────────────────────────

    fn sidebar_toggle(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        div()
            .id("toggle-sidebar")
            .track_focus(&self.toggle_focus)
            .tab_index(0)
            .w(px(26.0))
            .h(px(26.0))
            .flex_none()
            .rounded(px(6.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .focus_visible(|style| style.border_1().border_color(theme.accent))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon("icons/panel-left.svg", 14.0, theme.text_tertiary))
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .on_click(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.set_sidebar_visible(!this.sidebar_visible, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.set_sidebar_visible(!this.sidebar_visible, cx);
                    cx.stop_propagation();
                }
            }))
    }

    fn history_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        navigate_back: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        div()
            .id(id)
            .w(px(26.0))
            .h(px(26.0))
            .flex_none()
            .rounded(px(6.0))
            .flex()
            .items_center()
            .justify_center()
            .cursor_default()
            .opacity(0.35)
            .child(icon(icon_path, 14.0, theme.text_tertiary))
            .when(navigate_back, |element| {
                element
                    .opacity(1.0)
                    .hover(|element| element.bg(theme.overlay))
                    .active(|element| element.bg(theme.overlay_strong))
            })
    }

    fn render_sidebar_titlebar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("sidebar-titlebar")
            .h(px(TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .child(
                self.window_drag_region(
                    div()
                        .id("sidebar-traffic-light-drag-region")
                        .w(px(TRAFFIC_LIGHT_CLEARANCE))
                        .h_full()
                        .flex_none(),
                    cx,
                ),
            )
            .child(self.sidebar_toggle(cx))
            .child(
                div()
                    .ml(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .child(self.history_button("navigate-back", "icons/arrow-left.svg", false, cx))
                    .child(self.history_button(
                        "navigate-forward",
                        "icons/arrow-right.svg",
                        false,
                        cx,
                    )),
            )
            .child(self.window_drag_region(
                div().id("sidebar-titlebar-drag-region").h_full().flex_1(),
                cx,
            ))
    }

    fn menu_action_row(&self, page: MenuPage, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let selected = self.selected_page == page;
        let id = SharedString::from(format!(
            "sidebar-menu-{}",
            page.label().to_ascii_lowercase()
        ));
        div()
            .id(id)
            .tab_index(0)
            .w_full()
            .h(px(SIDEBAR_ACTION_ROW_HEIGHT))
            .flex_none()
            .px(px(4.0))
            .rounded(px(7.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .cursor_default()
            .focus_visible(|style| style.border_1().border_color(theme.accent))
            .when(selected, |element| {
                element.bg(theme.sidebar_item_background)
            })
            .hover(|element| element.bg(theme.sidebar_item_background))
            .active(|element| element.bg(theme.overlay_strong))
            .child(
                div()
                    .size(px(20.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon(
                        page.icon(),
                        16.0,
                        if selected {
                            theme.text
                        } else {
                            theme.text_secondary
                        },
                    )),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.0))
                    .text_color(if selected {
                        theme.text
                    } else {
                        theme.text_secondary
                    })
                    .child(page.label()),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_page(page, window, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.select_page(page, window, cx);
                    cx.stop_propagation();
                }
            }))
    }

    fn render_sidebar_footer(&self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        div()
            .flex_none()
            .h(px(FOOTER_HEIGHT))
            .px(px(10.0))
            .flex()
            .items_center()
            .text_size(px(11.0))
            .text_color(theme.text_ghost)
            .child("Hakata")
    }

    // ── Dropdowns ────────────────────────────────────────────────────────

    /// The dropdown chrome shared by every menu on the page: a focusable
    /// trigger that records its bounds and toggles the menu. Returns the
    /// element with its children attached by the caller.
    fn render_trigger(
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
    fn render_dropdown_card(
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

    /// The sidebar's device picker. Shows the selected device (or a hint to
    /// switch the default). Refreshed every few seconds from `adb devices`.
    fn render_device_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let has_selection = self.selected_device.is_some();
        let label = self
            .selected_device
            .clone()
            .unwrap_or_else(|| SharedString::from("No device"));
        let label_color = if has_selection {
            theme.text
        } else {
            theme.text_tertiary
        };

        let trigger = self
            .render_trigger(
                cx,
                "device-selector-trigger",
                &self.device_trigger_focus,
                self.device_trigger_bounds.clone(),
                Self::toggle_device_menu,
            )
            .child(icon("icons/smartphone.svg", 13.0, label_color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(label_color)
                    .child(label),
            )
            .child(
                icon("icons/chevron-down.svg", 12.0, theme.text_tertiary).when(
                    self.device_menu_open,
                    |icon| {
                        icon.with_transformation(gpui::Transformation::rotate(gpui::percentage(
                            0.5,
                        )))
                    },
                ),
            );

        let Some(trigger_bounds) = self.device_trigger_bounds.get() else {
            return trigger.into_any_element();
        };
        if !self.device_menu_open {
            return trigger.into_any_element();
        }

        let surface = self.render_dropdown_card(
            cx,
            "device-selector-card",
            trigger_bounds,
            self.device_trigger_bounds.clone(),
            Self::close_device_menu,
            232.0,
            |theme, cx| {
                if self.devices.is_empty() {
                    return div()
                        .mx(px(4.0))
                        .px(px(8.0))
                        .min_h(px(26.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .text_size(px(11.5))
                        .text_color(theme.text_tertiary)
                        .child(SharedString::from("No devices"));
                }
                let mut card = div();
                for device in &self.devices {
                    let serial = device.serial.clone();
                    let ready = device.state == "device";
                    let selected = self.selected_device.as_deref() == Some(serial.as_str());
                    let row_color = if !ready {
                        theme.text_ghost
                    } else if selected {
                        theme.text
                    } else {
                        theme.text_secondary
                    };
                    let row = div()
                        .id(SharedString::from(format!("device-menu-{}", serial)))
                        .mx(px(4.0))
                        .px(px(8.0))
                        .min_h(px(26.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .cursor_default()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(row_color)
                                .child(SharedString::from(serial.clone())),
                        )
                        .when(selected, |element| {
                            element.child(icon("icons/check.svg", 11.0, theme.text_tertiary))
                        })
                        .when(!ready, |element| {
                            element.child(
                                div()
                                    .flex_none()
                                    .text_size(px(10.0))
                                    .text_color(theme.text_ghost)
                                    .child(SharedString::from(device.state.clone())),
                            )
                        })
                        .when(ready, |element| {
                            element
                                .hover(|element| element.bg(theme.overlay))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener({
                                        let serial = serial.clone();
                                        move |this, _, _, cx| this.select_device(&serial, cx)
                                    }),
                                )
                        });
                    card = card.child(row);
                }
                card
            },
        );
        trigger.child(surface).into_any_element()
    }

    fn render_sidebar(&self, width: f32, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let is_resizing = self.panel_resize_drag.is_some();
        div()
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(if is_resizing {
                theme.sidebar_drag_background
            } else {
                theme.sidebar
            })
            .child(self.render_sidebar_titlebar(cx))
            .child(self.render_sidebar_body(cx))
    }

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

    fn render_sidebar_body(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px(px(10.0))
                    .pt(px(10.0))
                    .pb(px(8.0))
                    .child(self.render_device_selector(cx)),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(10.0))
                    .children(MenuPage::ALL.iter().map(|page| {
                        div()
                            .h(px(SIDEBAR_ACTION_ROW_HEIGHT))
                            .flex_none()
                            .child(self.menu_action_row(*page, cx))
                    })),
            )
            .child(div().flex_1())
            .child(self.render_sidebar_footer(cx))
    }

    // ── Panel resize ──────────────────────────────────────────────────────

    /// The draggable divider between two panes. `side` says which edge of the
    /// parent pane the strip straddles: the sidebar divider hangs off the
    /// content column's left edge, the Apps divider off the list panel's right
    /// edge.
    fn render_panel_resize_handle(
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
                (drag.start_width + delta).clamp(APPS_PANEL_MIN_WIDTH, viewport_width * APPS_PANEL_MAX_FRACTION)
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

    // ── Apps page ─────────────────────────────────────────────────────────

    fn render_apps_page(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
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

    fn render_settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .px(px(24.0))
            .pt(px(24.0))
            .child(
                div()
                    .w_full()
                    .max_w(px(480.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(10.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text_tertiary)
                            .child(SharedString::from("APPEARANCE")),
                    )
                    .child(
                        div()
                            .p(px(6.0))
                            .rounded(px(10.0))
                            .bg(theme.raised)
                            .border_1()
                            .border_color(theme.border)
                            .child(self.render_theme_selector(cx)),
                    ),
            )
            .into_any_element()
    }

    /// The appearance dropdown on the Settings page.
    fn render_theme_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let trigger = self
            .render_trigger(
                cx,
                "theme-selector-trigger",
                &self.theme_trigger_focus,
                self.theme_trigger_bounds.clone(),
                Self::toggle_theme_menu,
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child(SharedString::from("Theme")),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.0))
                    .text_color(theme.text)
                    .child(SharedString::from(self.theme_preference.label())),
            )
            .child(
                icon("icons/chevron-down.svg", 12.0, theme.text_tertiary).when(
                    self.theme_menu_open,
                    |icon| {
                        icon.with_transformation(gpui::Transformation::rotate(gpui::percentage(
                            0.5,
                        )))
                    },
                ),
            );

        let Some(trigger_bounds) = self.theme_trigger_bounds.get() else {
            return trigger.into_any_element();
        };
        if !self.theme_menu_open {
            return trigger.into_any_element();
        }

        let surface = self.render_dropdown_card(
            cx,
            "theme-selector-card",
            trigger_bounds,
            self.theme_trigger_bounds.clone(),
            Self::close_theme_menu,
            200.0,
            |theme, cx| {
                let mut card = div();
                for preference in ThemePreference::ALL {
                    let selected = self.theme_preference == preference;
                    card = card.child(
                        div()
                            .id(SharedString::from(format!(
                                "theme-menu-{}",
                                preference.label().to_ascii_lowercase()
                            )))
                            .mx(px(4.0))
                            .px(px(8.0))
                            .min_h(px(26.0))
                            .rounded(px(6.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .cursor_default()
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
                                    .child(SharedString::from(preference.label())),
                            )
                            .when(selected, |element| {
                                element.child(icon("icons/check.svg", 11.0, theme.text_tertiary))
                            })
                            .hover(|element| element.bg(theme.overlay))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.set_theme_preference(preference, cx)
                                }),
                            ),
                    );
                }
                card
            },
        );
        trigger.child(surface).into_any_element()
    }

    fn render_debug_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let adb_path = crate::adb::adb_path();
        let adb_path_text = adb_path.display().to_string();

        let copy_button = div()
            .id("debug-copy-adb-path")
            .tab_index(0)
            .focus_visible(|style| style.border_1().border_color(theme.accent))
            .h(px(26.0))
            .px(px(10.0))
            .rounded(px(6.0))
            .border_1()
            .border_color(theme.border_strong)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(5.0))
            .cursor_default()
            .text_size(px(10.5))
            .text_color(theme.text_secondary)
            .hover(|element| element.bg(theme.overlay))
            .child(icon("icons/copy.svg", 11.0, theme.text_tertiary))
            .child(SharedString::from("Copy"))
            .on_click(cx.listener({
                let path = adb_path_text.clone();
                move |_, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
                    cx.notify();
                }
            }));

        let path_value = div()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(10.5))
                    .text_color(theme.text_secondary)
                    .child(SharedString::from(adb_path_text.clone())),
            )
            .child(copy_button);

        let version_value: AnyElement = match &self.adb_version {
            None => div()
                .text_size(px(10.5))
                .text_color(theme.text_ghost)
                .child(SharedString::from("Not checked"))
                .into_any_element(),
            Some(AdbVersionStatus::Checking) => div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(10.5))
                .text_color(theme.text_tertiary)
                .child(
                    icon("icons/loader-circle.svg", 12.0, theme.text_tertiary)
                        .with_animation(
                            SharedString::from("debug-version-spinner"),
                            Animation::new(Duration::from_millis(900))
                                .repeat()
                                .with_easing(gpui::linear),
                            |icon, delta| {
                                icon.with_transformation(gpui::Transformation::rotate(
                                    gpui::percentage(delta),
                                ))
                            },
                        )
                        .into_any_element(),
                )
                .child(SharedString::from("Checking…"))
                .into_any_element(),
            Some(AdbVersionStatus::Version(version)) => div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(10.5))
                .text_color(theme.success)
                .child(icon("icons/check.svg", 11.0, theme.success))
                .child(SharedString::from(version.clone()))
                .into_any_element(),
            Some(AdbVersionStatus::Error(error)) => div()
                .text_size(px(10.5))
                .text_color(theme.danger)
                .child(SharedString::from(error.clone()))
                .into_any_element(),
        };

        let device_value: AnyElement = match &self.selected_device {
            Some(serial) => div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(px(10.5))
                .text_color(theme.text_secondary)
                .child(icon("icons/smartphone.svg", 12.0, theme.text_secondary))
                .child(SharedString::from(serial.clone()))
                .into_any_element(),
            None => div()
                .text_size(px(10.5))
                .text_color(theme.text_ghost)
                .child(SharedString::from("No device"))
                .into_any_element(),
        };

        let card = div()
            .mt(px(15.0))
            .w_full()
            .rounded(px(13.0))
            .bg(theme.raised)
            .overflow_hidden()
            .child(debug_info_row(
                &theme,
                "adb path",
                path_value.into_any_element(),
                false,
            ))
            .child(debug_info_row(&theme, "adb version", version_value, false))
            .child(debug_info_row(&theme, "device", device_value, true));

        div()
            .id("debug-page-scroll")
            .size_full()
            .overflow_y_scroll()
            .px(px(32.0))
            .child(
                div()
                    .w_full()
                    .max_w(px(760.0))
                    .mx_auto()
                    .child(
                        div()
                            .pt(px(2.0))
                            .flex_none()
                            .text_size(px(18.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(SharedString::from("Debug")),
                    )
                    .child(card),
            )
            .into_any_element()
    }

    /// Non-closable overlay shown while adb is bootstrapped on first launch.
    fn render_adb_bootstrap_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = Theme::current(cx);
        let (title, body) = match &self.adb_bootstrap {
            Some(AdbBootstrapState::Downloading { progress }) => {
                let spinner = icon("icons/loader-circle.svg", 14.0, theme.text_tertiary)
                    .with_animation(
                        SharedString::from("adb-bootstrap-spinner"),
                        Animation::new(Duration::from_millis(900))
                            .repeat()
                            .with_easing(gpui::linear),
                        |icon, delta| {
                            icon.with_transformation(gpui::Transformation::rotate(
                                gpui::percentage(delta),
                            ))
                        },
                    )
                    .into_any_element();
                let title_row = div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(spinner)
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(SharedString::from("Downloading adb")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(theme.text_tertiary)
                            .child(SharedString::from(format!("{:.0}%", progress * 100.0))),
                    );
                let bar = div()
                    .mt(px(2.0))
                    .h(px(8.0))
                    .w_full()
                    .rounded(px(4.0))
                    .bg(theme.surface)
                    .child(
                        div()
                            .h_full()
                            .w(DefiniteLength::Fraction(*progress))
                            .rounded(px(4.0))
                            .bg(theme.accent),
                    );
                (title_row, bar)
            }
            Some(AdbBootstrapState::Error(error)) => {
                let retry = div()
                    .id("adb-bootstrap-retry")
                    .tab_index(0)
                    .cursor_default()
                    .h(px(28.0))
                    .px(px(14.0))
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(theme.border_strong)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.5))
                    .text_color(theme.text_secondary)
                    .hover(|element| element.bg(theme.overlay))
                    .focus_visible(|style| style.border_1().border_color(theme.accent))
                    .on_click(cx.listener(|this, _, _, cx| this.retry_adb_bootstrap(cx)))
                    .child(SharedString::from("Retry"));
                (
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text)
                        .child(SharedString::from("Download failed")),
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .child(
                            div()
                                .w_full()
                                .text_size(px(11.0))
                                .line_height(px(16.0))
                                .text_color(theme.danger)
                                .child(SharedString::from(error.clone())),
                        )
                        .child(div().flex().justify_end().child(retry)),
                )
            }
            _ => return None,
        };

        let scrim = if theme.is_dark {
            gpui::hsla(0.0, 0.0, 0.0, 0.34)
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.16)
        };
        let card = div()
            .w(px(380.0))
            .rounded(px(13.0))
            .bg(theme.raised)
            .border_1()
            .border_color(theme.border)
            .px(px(24.0))
            .py(px(20.0))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(title)
            .child(body);
        let layer = div()
            .id("adb-bootstrap-layer")
            .absolute()
            .inset_0()
            .occlude()
            .bg(scrim)
            .p(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .child(card);
        Some(gpui::deferred(layer).with_priority(4).into_any_element())
    }
}

/// A waku-settings-style label/value row inside a raised card.
fn debug_info_row(theme: &Theme, label: &str, value: AnyElement, last: bool) -> Div {
    div()
        .px(px(20.0))
        .py(px(14.0))
        .when(!last, |element| {
            element.border_b_1().border_color(theme.border)
        })
        .flex()
        .items_center()
        .gap(px(12.0))
        .child(
            div()
                .w(px(84.0))
                .flex_none()
                .text_size(px(10.5))
                .text_color(theme.text_tertiary)
                .child(SharedString::from(label)),
        )
        .child(div().flex_1().min_w_0().child(value))
}

/// A monochrome icon from the embedded set, tinted via text color.
pub fn icon(path: &'static str, size: f32, color: gpui::Hsla) -> Svg {
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
        root.into_any_element()
    }
}
