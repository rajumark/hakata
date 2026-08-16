use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, ClipboardItem, Context, DefiniteLength, Div, FontWeight,
    InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, px, prelude::*,
};

use crate::theme::Theme;

use super::{Hakata, MenuPage, icon};

/// The adb version probe run when the Debug page is opened.
pub(crate) enum AdbVersionStatus {
    Checking,
    Version(String),
    Error(String),
}

/// The launch-time adb bootstrap: download platform-tools on first run.
pub(crate) enum AdbBootstrapState {
    Downloading { progress: f32 },
    Error(String),
    Done,
}

impl Hakata {
    /// Probe `adb version` on the background executor the first time the Debug
    /// page is shown. Rendering only ever reads the cached status.
    pub(crate) fn check_adb_version(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn start_adb_bootstrap(&mut self, cx: &mut Context<Self>) {
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

    // ── Debug page ────────────────────────────────────────────────────────

    pub(crate) fn render_debug_page(&self, cx: &mut Context<Self>) -> AnyElement {
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
    pub(crate) fn render_adb_bootstrap_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
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
