use gpui::{
    AnyElement, Context, Div, FontWeight, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, ParentElement, SharedString, Stateful, Styled, div, px, prelude::*,
};

use crate::theme::Theme;

use super::{Hakata, icon};

pub(crate) const PREFERENCES_SIDEBAR_WIDTH: f32 = 208.0;
const PREFERENCES_CONTENT_MAX_WIDTH: f32 = 520.0;
const PREFERENCES_MENU_ROW_HEIGHT: f32 = 28.0;

/// The left-hand sections of the Preferences page. Each shows its own page in
/// the content pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreferencesPage {
    Theme,
    Alerts,
    Shortcut,
    About,
}

impl PreferencesPage {
    pub(crate) const ALL: [Self; 4] = [Self::Theme, Self::Alerts, Self::Shortcut, Self::About];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Alerts => "Alerts",
            Self::Shortcut => "Shortcut",
            Self::About => "About",
        }
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Theme => "icons/appearance.svg",
            Self::Alerts => "icons/alert.svg",
            Self::Shortcut => "icons/settings.svg",
            Self::About => "icons/info.svg",
        }
    }
}

impl Hakata {
    /// The Preferences page: a sidebar of sections on the left and the
    /// selected section's page on the right, like the Apps screen.
    pub(crate) fn render_preferences_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        div()
            .id("preferences-page")
            .size_full()
            .flex()
            .border_t_1()
            .border_color(theme.sidebar_border)
            .child(self.render_preferences_sidebar(cx))
            .child(self.render_preferences_content(cx))
            .into_any_element()
    }

    fn render_preferences_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        div()
            .w(px(PREFERENCES_SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                div()
                    .flex_none()
                    .px(px(14.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_size(px(10.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .child(SharedString::from("PREFERENCES")),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(10.0))
                    .children(PreferencesPage::ALL.iter().map(|page| {
                        div()
                            .h(px(PREFERENCES_MENU_ROW_HEIGHT))
                            .flex_none()
                            .child(self.render_preferences_menu_row(*page, cx))
                    })),
            )
            .child(div().flex_1())
    }

    fn render_preferences_menu_row(
        &self,
        page: PreferencesPage,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = Theme::current(cx);
        let selected = self.selected_preferences_page == page;
        div()
            .id(SharedString::from(format!(
                "preferences-menu-{}",
                page.label().to_ascii_lowercase()
            )))
            .tab_index(0)
            .w_full()
            .h(px(PREFERENCES_MENU_ROW_HEIGHT))
            .flex_none()
            .px(px(8.0))
            .rounded(px(6.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_default()
            .focus_visible(|style| style.border_1().border_color(theme.accent))
            .when(selected, |element| element.bg(theme.sidebar_item_background))
            .hover(|element| element.bg(theme.overlay))
            .active(|element| element.bg(theme.overlay_strong))
            .child(icon(
                page.icon(),
                15.0,
                if selected {
                    theme.text_secondary
                } else {
                    theme.text_tertiary
                },
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.5))
                    .text_color(if selected {
                        theme.text
                    } else {
                        theme.text_secondary
                    })
                    .child(SharedString::from(page.label())),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected_preferences_page = page;
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.selected_preferences_page = page;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
    }

    fn render_preferences_content(&self, cx: &mut Context<Self>) -> Div {
        let theme = Theme::current(cx);
        let page = self.selected_preferences_page;
        div()
            .flex_1()
            .h_full()
            .min_w_0()
            .flex()
            .flex_col()
            .pt(px(14.0))
            .px(px(16.0))
            .child(
                div()
                    .flex_none()
                    .min_w_0()
                    .truncate()
                    .text_size(px(15.0))
                    .text_color(theme.text)
                    .child(SharedString::from(page.label())),
            )
            .child(div().h(px(10.0)))
            .child(
                div()
                    .id("preferences-content-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(match page {
                        PreferencesPage::Theme => self.render_theme_settings(cx),
                        page => self.render_placeholder(page, cx),
                    }),
            )
    }

    /// The Theme page: a Waku-style Appearance card with a Theme and a
    /// Language row, each a label + description on the left and a dropdown on
    /// the right.
    fn render_theme_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let theme_selector = div().w(px(180.0)).child(self.render_theme_selector(cx));
        let language_selector = div().w(px(180.0)).child(self.render_language_selector(cx));

        div()
            .w_full()
            .max_w(px(PREFERENCES_CONTENT_MAX_WIDTH))
            .mt(px(15.0))
            .flex()
            .flex_col()
            .rounded(px(13.0))
            .overflow_hidden()
            .bg(theme.raised)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(SharedString::from("Theme")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(px(12.5))
                                    .line_height(px(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(SharedString::from(
                                        "Choose how the app looks and matches your system.",
                                    )),
                            ),
                    )
                    .child(theme_selector),
            )
            .child(div().mx(px(20.0)).h(px(1.0)).bg(theme.border))
            .child(
                div()
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(24.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(SharedString::from("Language")),
                            )
                            .child(
                                div()
                                    .mt(px(5.0))
                                    .text_size(px(12.5))
                                    .line_height(px(18.0))
                                    .text_color(theme.text_secondary)
                                    .child(SharedString::from(
                                        "Choose the language used in the interface.",
                                    )),
                            ),
                    )
                    .child(language_selector),
            )
            .into_any_element()
    }

    /// The Language dropdown on the Theme page, styled like the theme
    /// selector.
    fn render_language_selector(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let trigger = self
            .render_trigger(
                cx,
                "language-selector-trigger",
                &self.language_trigger_focus,
                self.language_trigger_bounds.clone(),
                Self::toggle_language_menu,
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.0))
                    .text_color(theme.text_secondary)
                    .child(SharedString::from("Language")),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.0))
                    .text_color(theme.text)
                    .child(SharedString::from("English")),
            )
            .child(
                icon("icons/chevron-down.svg", 12.0, theme.text_tertiary).when(
                    self.language_menu_open,
                    |icon| {
                        icon.with_transformation(gpui::Transformation::rotate(gpui::percentage(
                            0.5,
                        )))
                    },
                ),
            );

        let Some(trigger_bounds) = self.language_trigger_bounds.get() else {
            return trigger.into_any_element();
        };
        if !self.language_menu_open {
            return trigger.into_any_element();
        }

        let surface = self.render_dropdown_card(
            cx,
            "language-selector-card",
            trigger_bounds,
            self.language_trigger_bounds.clone(),
            Self::close_language_menu,
            200.0,
            |theme, cx| {
                div().child(
                    div()
                        .id("language-menu-english")
                        .mx(px(4.0))
                        .px(px(8.0))
                        .min_h(px(26.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .cursor_default()
                        .hover(|element| element.bg(theme.overlay))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.5))
                                .text_color(theme.text)
                                .child(SharedString::from("English")),
                        )
                        .child(icon("icons/check.svg", 11.0, theme.text_tertiary))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.close_language_menu(cx)),
                        ),
                )
            },
        );
        trigger.child(surface).into_any_element()
    }

    pub(crate) fn toggle_language_menu(&mut self, cx: &mut Context<Self>) {
        self.language_menu_open = !self.language_menu_open;
        cx.notify();
    }

    pub(crate) fn close_language_menu(&mut self, cx: &mut Context<Self>) {
        if self.language_menu_open {
            self.language_menu_open = false;
            cx.notify();
        }
    }

    fn render_placeholder(&self, page: PreferencesPage, cx: &mut Context<Self>) -> AnyElement {
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
                    .child(SharedString::from(page.label())),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_ghost)
                    .child(SharedString::from("coming soon")),
            )
            .into_any_element()
    }
}
