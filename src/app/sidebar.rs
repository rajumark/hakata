use gpui::{
    AnyElement, Div, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement,
    SharedString, Stateful, Styled, div, px, prelude::*,
};

use crate::theme::Theme;

use super::{Hakata, MenuPage, TRAFFIC_LIGHT_CLEARANCE, icon};

const TITLEBAR_HEIGHT: f32 = 48.0;
const FOOTER_HEIGHT: f32 = 40.0;
const SIDEBAR_ACTION_ROW_HEIGHT: f32 = 32.0;

impl Hakata {
    pub(crate) fn sidebar_toggle(&self, cx: &mut Context<Self>) -> Stateful<Div> {
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

    pub(crate) fn history_button(
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

    pub(crate) fn render_sidebar(&self, width: f32, cx: &mut Context<Self>) -> Div {
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
}
