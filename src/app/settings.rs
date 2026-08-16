use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement,
    SharedString, Styled, div, px, prelude::*,
};

use crate::theme::{Theme, ThemePreference};

use super::{Hakata, icon};

impl Hakata {
    pub(crate) fn render_settings_page(&self, cx: &mut Context<Self>) -> AnyElement {
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
    pub(crate) fn render_theme_selector(&self, cx: &mut Context<Self>) -> AnyElement {
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
}
