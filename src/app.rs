use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, ClipboardItem, Context, DefiniteLength, Div, Entity,
    FocusHandle, FontWeight, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Render, SharedString, Stateful,
    Styled, Svg, Window, div, prelude::*, px,
};

use crate::theme::Theme;

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

/// The three sidebar menus. Each shows a page in the main area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuPage {
    NewTask,
    Search,
    Settings,
    Debug,
}

impl MenuPage {
    const ALL: [Self; 4] = [Self::NewTask, Self::Search, Self::Settings, Self::Debug];

    fn label(self) -> &'static str {
        match self {
            Self::NewTask => "New Task",
            Self::Search => "Search",
            Self::Settings => "Settings",
            Self::Debug => "Debug",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::NewTask => "icons/compose.svg",
            Self::Search => "icons/search.svg",
            Self::Settings => "icons/settings.svg",
            Self::Debug => "icons/terminal-square.svg",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PanelResizeDrag {
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

pub struct Hakata {
    selected_page: MenuPage,
    sidebar_visible: bool,
    sidebar_width: f32,
    panel_resize_drag: Option<PanelResizeDrag>,
    header_drag_armed: bool,
    toggle_focus: FocusHandle,
    adb_version: Option<AdbVersionStatus>,
    adb_bootstrap: Option<AdbBootstrapState>,
}

impl Hakata {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            selected_page: MenuPage::NewTask,
            sidebar_visible: true,
            sidebar_width: SIDEBAR_DEFAULT_WIDTH,
            panel_resize_drag: None,
            header_drag_armed: false,
            toggle_focus: cx.focus_handle(),
            adb_version: None,
            adb_bootstrap: None,
        })
    }

    fn effective_sidebar_width(&self, window: &Window) -> f32 {
        if !self.sidebar_visible {
            return 0.0;
        }
        let viewport_width = f32::from(window.viewport_size().width);
        self.sidebar_width
            .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH.min(viewport_width - MAIN_PANEL_MIN_WIDTH).max(SIDEBAR_MIN_WIDTH))
    }

    fn select_page(&mut self, page: MenuPage, cx: &mut Context<Self>) {
        self.selected_page = page;
        if page == MenuPage::Debug {
            self.check_adb_version(cx);
        }
        cx.notify();
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
                .spawn(async move { std::process::Command::new(&adb_path).arg("version").output() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.adb_version = Some(match result {
                    Ok(output) if output.status.success() => {
                        AdbVersionStatus::Version(
                            String::from_utf8_lossy(&output.stdout).trim().to_string(),
                        )
                    }
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
                    .child(self.history_button("navigate-forward", "icons/arrow-right.svg", false, cx)),
            )
            .child(self.window_drag_region(
                div().id("sidebar-titlebar-drag-region").h_full().flex_1(),
                cx,
            ))
    }

    fn menu_action_row(&self, page: MenuPage, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let selected = self.selected_page == page;
        let id = SharedString::from(format!("sidebar-menu-{}", page.label().to_ascii_lowercase()));
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
            .when(selected, |element| element.bg(theme.sidebar_item_background))
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
            .on_click(cx.listener(move |this, _, _, cx| {
                this.select_page(page, cx);
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.select_page(page, cx);
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

    fn render_panel_resize_handle(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let active = self.panel_resize_drag.is_some();
        let strip_left = -5.0;
        let strip_width = 10.0;
        div()
            .id("sidebar-resize-handle")
            .absolute()
            .top_0()
            .left(px(strip_left))
            .w(px(strip_width))
            .h_full()
            .group("panel-resize-handle")
            .cursor_col_resize()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(5.0))
                    .w(px(2.0))
                    .h_full()
                    .bg(if active {
                        theme.resize_handle
                    } else {
                        gpui::transparent_black()
                    })
                    .group_hover("panel-resize-handle", |element| {
                        element.bg(theme.resize_handle)
                    }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event, window, cx| {
                    this.begin_panel_resize(event, window, cx);
                }),
            )
    }

    fn begin_panel_resize(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start_width = self.effective_sidebar_width(window);
        self.sidebar_width = start_width;
        self.panel_resize_drag = Some(PanelResizeDrag {
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
        let maximum = SIDEBAR_MAX_WIDTH
            .min(viewport_width - MAIN_PANEL_MIN_WIDTH)
            .max(SIDEBAR_MIN_WIDTH);
        let width = (drag.start_width + delta).clamp(SIDEBAR_MIN_WIDTH, maximum);
        if (self.sidebar_width - width).abs() < 0.5 {
            return;
        }
        self.sidebar_width = width;
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

    fn render_page(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.selected_page {
            MenuPage::Debug => self.render_debug_page(cx),
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
                            .child(SharedString::from(format!(
                                "{} coming soon",
                                page.label()
                            ))),
                    )
                    .into_any_element()
            }
        }
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
            .child(debug_info_row(&theme, "adb version", version_value, true));

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
        .when(!last, |element| element.border_b_1().border_color(theme.border))
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
                            .child(self.render_page(cx)),
                    )
                    .when(self.sidebar_visible, |element| {
                        element.child(self.render_panel_resize_handle(cx))
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
