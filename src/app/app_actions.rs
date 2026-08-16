use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, ClipboardItem, Context, DefiniteLength, Div, FontWeight,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Point, SharedString, Stateful,
    Styled, div, px, prelude::*,
};

use crate::theme::Theme;

use super::{Hakata, icon};

/// The actions offered on a package row's context menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AppAction {
    Open,
    ForceStop,
    Restart,
    ClearData,
    Uninstall,
    Copy,
    AppInfo,
    PlayStore,
    Enable,
    Disable,
    GrantAll,
    RevokeAll,
    ManagePermissions,
    Pin,
    Unpin,
}

impl AppAction {
    pub(crate) fn label(self) -> String {
        match self {
            Self::Open => tr!("action.open"),
            Self::ForceStop => tr!("action.force_stop"),
            Self::Restart => tr!("action.restart"),
            Self::ClearData => tr!("action.clear_data"),
            Self::Uninstall => tr!("action.uninstall"),
            Self::Copy => tr!("action.copy"),
            Self::AppInfo => tr!("action.app_info"),
            Self::PlayStore => tr!("action.play_store"),
            Self::Enable => tr!("action.enable"),
            Self::Disable => tr!("action.disable"),
            Self::GrantAll => tr!("action.grant_all"),
            Self::RevokeAll => tr!("action.revoke_all"),
            Self::ManagePermissions => tr!("action.manage_permissions"),
            Self::Pin => tr!("action.pin"),
            Self::Unpin => tr!("action.unpin"),
        }
    }

    fn destructive(self) -> bool {
        matches!(self, Self::ClearData | Self::Uninstall)
    }
}

/// The open context menu: which package it was opened on and where the mouse
/// was when the row was right-clicked.
pub(crate) struct PackageContextMenu {
    pub(crate) package: SharedString,
    pub(crate) position: Point<gpui::Pixels>,
    pub(crate) more_open: bool,
}

/// A destructive action waiting for the user's confirmation.
pub(crate) struct ConfirmationRequest {
    pub(crate) action: AppAction,
    pub(crate) package: SharedString,
}

/// The outcome of the most recent package action, shown as a status line.
#[derive(Clone, Debug)]
pub(crate) enum AppActionStatus {
    Running { message: String },
    Done { message: String },
    Error { message: String },
}

/// A grant/revoke-all run in progress, driving the progress dialog.
pub(crate) struct PermissionRun {
    pub(crate) package: SharedString,
    pub(crate) grant: bool,
    pub(crate) total: usize,
    pub(crate) done: usize,
    pub(crate) errors: Vec<String>,
}

const MENU_WIDTH: f32 = 212.0;
const MAIN_ACTIONS: [AppAction; 6] = [
    AppAction::Open,
    AppAction::ForceStop,
    AppAction::Restart,
    AppAction::ClearData,
    AppAction::Uninstall,
    AppAction::Copy,
];
const MORE_ACTIONS: [AppAction; 7] = [
    AppAction::AppInfo,
    AppAction::PlayStore,
    AppAction::Enable,
    AppAction::Disable,
    AppAction::GrantAll,
    AppAction::RevokeAll,
    AppAction::ManagePermissions,
];

/// One or more adb invocations that carry out an action.
enum CommandPlan {
    Host(Vec<String>),
    Shell(Vec<Vec<String>>),
}

fn command_plan(action: AppAction, package: &str) -> CommandPlan {
    let launch = || {
        vec![
            "monkey".to_string(),
            "-p".to_string(),
            package.to_string(),
            "-c".to_string(),
            "android.intent.category.LAUNCHER".to_string(),
            "1".to_string(),
        ]
    };
    match action {
        AppAction::Open => CommandPlan::Shell(vec![launch()]),
        AppAction::ForceStop => {
            CommandPlan::Shell(vec![vec!["am".into(), "force-stop".into(), package.into()]])
        }
        AppAction::Restart => CommandPlan::Shell(vec![
            vec!["am".into(), "force-stop".into(), package.into()],
            launch(),
        ]),
        AppAction::ClearData => {
            CommandPlan::Shell(vec![vec!["pm".into(), "clear".into(), package.into()]])
        }
        AppAction::Uninstall => CommandPlan::Host(vec!["uninstall".into(), package.into()]),
        AppAction::AppInfo => CommandPlan::Shell(vec![vec![
            "am".into(),
            "start".into(),
            "-a".into(),
            "android.settings.APPLICATION_DETAILS_SETTINGS".into(),
            "-d".into(),
            format!("package:{}", package),
        ]]),
        AppAction::PlayStore => CommandPlan::Shell(vec![vec![
            "am".into(),
            "start".into(),
            "-a".into(),
            "android.intent.action.VIEW".into(),
            "-d".into(),
            format!("https://play.google.com/store/apps/details?id={}", package),
        ]]),
        AppAction::Enable => {
            CommandPlan::Shell(vec![vec!["pm".into(), "enable".into(), package.into()]])
        }
        AppAction::Disable => {
            CommandPlan::Shell(vec![vec!["pm".into(), "disable-user".into(), package.into()]])
        }
        AppAction::ManagePermissions => CommandPlan::Shell(vec![vec![
            "am".into(),
            "start".into(),
            "-a".into(),
            "android.intent.action.MANAGE_APP_PERMISSIONS".into(),
            "-d".into(),
            format!("package:{}", package),
        ]]),
        _ => CommandPlan::Shell(Vec::new()),
    }
}

impl Hakata {
    /// Select a package, clearing the title selection and fetching its
    /// `dumpsys` dump. Shared by the left-click and right-click row handlers.
    pub(crate) fn select_package(&mut self, package: SharedString, cx: &mut Context<Self>) {
        self.selected_package = Some(package);
        self.title_selected = false;
        self.fetch_package_dump(false, cx);
        cx.notify();
    }

    /// Open the right-click context menu at the mouse position.
    pub(crate) fn open_package_context_menu(
        &mut self,
        package: SharedString,
        position: Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.package_context_menu = Some(PackageContextMenu {
            package,
            position,
            more_open: false,
        });
        cx.notify();
    }

    pub(crate) fn close_package_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.package_context_menu.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn toggle_context_more(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.package_context_menu {
            menu.more_open = !menu.more_open;
            cx.notify();
        }
    }

    /// Run a menu action against `package`, then close any open menu.
    pub(crate) fn dispatch_package_action(
        &mut self,
        action: AppAction,
        package: SharedString,
        cx: &mut Context<Self>,
    ) {
        self.package_context_menu = None;
        self.run_package_action(package, action, cx);
    }

    fn run_package_action(
        &mut self,
        package: SharedString,
        action: AppAction,
        cx: &mut Context<Self>,
    ) {
        self.app_action_epoch += 1;
        match action {
            AppAction::Copy => {
                cx.write_to_clipboard(ClipboardItem::new_string(package.to_string()));
                self.report_done(tr!("action.copied_package", package = package));
            }
            AppAction::Pin => {
                if !self.pinned_apps.contains(&package) {
                    self.pinned_apps.push(package.clone());
                }
                self.save_settings();
            }
            AppAction::Unpin => {
                self.pinned_apps.retain(|p| p != &package);
                self.save_settings();
            }
            AppAction::ClearData | AppAction::Uninstall => {
                self.confirm_request = Some(ConfirmationRequest { action, package });
            }
            AppAction::GrantAll | AppAction::RevokeAll => {
                self.start_permission_run(package, action == AppAction::GrantAll, cx);
            }
            _ => self.run_simple_package_action(package, action, cx),
        }
        cx.notify();
    }

    pub(crate) fn confirm_destructive_action(&mut self, cx: &mut Context<Self>) {
        if let Some(request) = self.confirm_request.take() {
            self.run_package_action(request.package, request.action, cx);
        }
    }

    pub(crate) fn cancel_confirmation(&mut self, cx: &mut Context<Self>) {
        self.confirm_request = None;
        cx.notify();
    }

    /// A one-shot adb action. Runs on the background executor; the status
    /// line reports the outcome and package state refreshes when the action
    /// can change it.
    fn run_simple_package_action(
        &mut self,
        package: SharedString,
        action: AppAction,
        cx: &mut Context<Self>,
    ) {
        let Some(serial) = self.selected_device.clone() else {
            self.report_error(tr!("common.no_device_selected"));
            return;
        };
        let plan = command_plan(action, package.as_str());
        let epoch = self.app_action_epoch;
        self.app_action_status = Some(AppActionStatus::Running {
            message: tr!("action.running", action = action.label()),
        });
        cx.notify();
        let adb_path = crate::adb::adb_path();
        let serial_for_spawn = serial.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match plan {
                        CommandPlan::Host(args) => {
                            let mut command = std::process::Command::new(&adb_path);
                            command.args(&args);
                            match command.output() {
                                Ok(output) if output.status.success() => Ok(()),
                                Ok(output) => {
                                    let message = String::from_utf8_lossy(&output.stderr)
                                        .trim()
                                        .to_string();
                                    Err(if message.is_empty() {
                                        tr!("action.failed", action = action.label())
                                    } else {
                                        message
                                    })
                                }
                                Err(error) => Err(error.to_string()),
                            }
                        }
                        CommandPlan::Shell(commands) => {
                            let mut last_error = None;
                            for args in commands {
                                let output = std::process::Command::new(&adb_path)
                                    .arg("-s")
                                    .arg(serial_for_spawn.as_str())
                                    .arg("shell")
                                    .args(&args)
                                    .output();
                                match output {
                                    Ok(output) if output.status.success() => {}
                                    Ok(output) => {
                                        let message = String::from_utf8_lossy(&output.stderr)
                                            .trim()
                                            .to_string();
                                        last_error = Some(if message.is_empty() {
                                            tr!("action.failed", action = action.label())
                                        } else {
                                            message
                                        });
                                        break;
                                    }
                                    Err(error) => {
                                        last_error = Some(error.to_string());
                                        break;
                                    }
                                }
                            }
                            match last_error {
                                Some(message) => Err(message),
                                None => Ok(()),
                            }
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.app_action_epoch != epoch {
                    return;
                }
                match result {
                    Ok(()) => this.report_done(tr!("action.complete", action = action.label())),
                    Err(message) => this.report_error(message),
                }
                if matches!(
                    action,
                    AppAction::Uninstall
                        | AppAction::ClearData
                        | AppAction::Enable
                        | AppAction::Disable
                ) {
                    this.refresh_packages(true, cx);
                }
                if action == AppAction::Uninstall {
                    this.selected_package = None;
                    this.package_dump_raw = None;
                    this.package_dump_error = None;
                    this.permissions.clear();
                } else {
                    this.fetch_package_dump(true, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Grant (or revoke) every requested permission for a package, streaming
    /// progress into the non-dismissible dialog. Refreshes the dump when done.
    pub(crate) fn start_permission_run(
        &mut self,
        package: SharedString,
        grant: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(serial) = self.selected_device.clone() else {
            self.report_error(tr!("common.no_device_selected"));
            return;
        };
        let permissions = self.permissions.clone();
        if permissions.is_empty() {
            self.report_error(tr!(if grant {
                "action.no_requested_grant"
            } else {
                "action.no_requested_revoke"
            }));
            return;
        }
        let total = permissions.len();
        self.permission_run = Some(PermissionRun {
            package: package.clone(),
            grant,
            total,
            done: 0,
            errors: Vec::new(),
        });
        cx.notify();

        let (sender, receiver) = smol::channel::bounded::<(usize, Option<String>)>(8);
        let adb_path = crate::adb::adb_path();
        let serial_for_spawn = serial.clone();
        let package_for_spawn = package.clone();
        cx.spawn(async move |this, cx| {
            let task = cx.background_executor().spawn(async move {
                let mut done = 0usize;
                for permission in &permissions {
                    let output = std::process::Command::new(&adb_path)
                        .arg("-s")
                        .arg(serial_for_spawn.as_str())
                        .arg("shell")
                        .arg("pm")
                        .arg(if grant { "grant" } else { "revoke" })
                        .arg(package_for_spawn.as_str())
                        .arg(permission.as_str())
                        .output();
                    done += 1;
                    let error = match output {
                        Ok(output) if output.status.success() => None,
                        Ok(output) => {
                            let message =
                                String::from_utf8_lossy(&output.stderr).trim().to_string();
                            Some(if message.is_empty() {
                                tr!(if grant {
                                    "permissions.grant_failed"
                                } else {
                                    "permissions.revoke_failed"
                                }, name = permission)
                            } else {
                                message
                            })
                        }
                        Err(error) => Some(error.to_string()),
                    };
                    let _ = sender.send_blocking((done, error));
                }
            });
            while let Ok((done, error)) = receiver.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(run) = &mut this.permission_run {
                        run.done = done;
                        if let Some(error) = error.clone() {
                            run.errors.push(error);
                        }
                    }
                    cx.notify();
                });
            }
            let _ = task.await;
            let _ = this.update(cx, |this, cx| {
                let errors = this
                    .permission_run
                    .take()
                    .map(|run| run.errors.len())
                    .unwrap_or(0);
                this.fetch_package_dump(true, cx);
                if errors == 0 {
                    this.report_done(tr!(
                        if grant { "action.granted_all" } else { "action.revoked_all" },
                        package = package,
                        total = total
                    ));
                } else {
                    this.report_error(tr!("action.some_failed", errors = errors, total = total));
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn report_done(&mut self, message: String) {
        self.app_action_status = Some(AppActionStatus::Done { message });
    }

    fn report_error(&mut self, message: String) {
        self.app_action_status = Some(AppActionStatus::Error { message });
    }

    pub(crate) fn save_settings(&self) {
        let _ = crate::settings::save(&crate::settings::Settings {
            theme: self.theme_preference,
            language: self.language_preference,
            pinned_apps: self.pinned_apps.iter().map(|p| p.to_string()).collect(),
            apps_filter: self.apps_filter,
        });
    }

    // ── Context menu ─────────────────────────────────────────────────────

    pub(crate) fn render_package_context_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(menu) = &self.package_context_menu else {
            return None;
        };
        let theme = Theme::current(cx);
        let pinned = self.pinned_apps.contains(&menu.package);
        let mut card = div()
            .id("package-context-menu")
            .occlude()
            .w(px(MENU_WIDTH))
            .py(px(4.0))
            .rounded(px(9.0))
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.raised)
            .shadow_lg()
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.close_package_context_menu(cx);
            }));
        for action in MAIN_ACTIONS {
            card = card.child(context_menu_item(&theme, action, menu.package.clone(), cx));
        }
        card = card.child(menu_divider(theme.border));
        card = card.child(context_menu_more_row(&theme, menu.more_open, cx));
        if menu.more_open {
            card = card.child(menu_divider(theme.border));
            for action in MORE_ACTIONS {
                card = card.child(context_menu_item(&theme, action, menu.package.clone(), cx));
            }
        }
        card = card.child(menu_divider(theme.border));
        card = card.child(context_menu_item(
            &theme,
            if pinned { AppAction::Unpin } else { AppAction::Pin },
            menu.package.clone(),
            cx,
        ));
        Some(
            gpui::deferred(gpui::anchored().position(menu.position).child(card))
                .with_priority(4)
                .into_any_element(),
        )
    }

    // ── Confirmation dialog ──────────────────────────────────────────────

    pub(crate) fn render_confirmation_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let Some(request) = &self.confirm_request else {
            return None;
        };
        let theme = Theme::current(cx);
        let verb = match request.action {
            AppAction::Uninstall => tr!("action.confirm_uninstall", package = request.package),
            AppAction::ClearData => tr!("action.confirm_clear_data", package = request.package),
            _ => request.action.label().to_lowercase(),
        };
        let title_row = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text)
                    .child(request.action.label()),
            )
            .child(div().flex_1());

        let cancel = div()
            .id("confirm-cancel")
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
            .on_click(cx.listener(|this, _, _, cx| this.cancel_confirmation(cx)))
            .child(tr_cow!("action.cancel"));

        let confirm = div()
            .id("confirm-ok")
            .tab_index(0)
            .cursor_default()
            .h(px(28.0))
            .px(px(14.0))
            .rounded(px(7.0))
            .bg(theme.danger)
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(11.5))
            .text_color(gpui::Hsla::from(gpui::Rgba {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            }))
            .hover(|element| element.bg(theme.danger))
            .active(|element| element.opacity(0.85))
            .focus_visible(|style| style.border_1().border_color(theme.accent))
            .on_click(cx.listener(|this, _, _, cx| this.confirm_destructive_action(cx)))
            .child(request.action.label());

        let card = div()
            .w(px(360.0))
            .rounded(px(13.0))
            .bg(theme.raised)
            .border_1()
            .border_color(theme.border)
            .px(px(24.0))
            .py(px(20.0))
            .flex()
            .flex_col()
            .gap(px(14.0))
            .child(title_row)
            .child(
                div()
                    .w_full()
                    .text_size(px(11.5))
                    .line_height(px(17.0))
                    .text_color(theme.text_secondary)
                    .child(tr!("action.confirm_body", verb = verb)),
            )
            .child(div().flex().justify_end().gap(px(8.0)).child(cancel).child(confirm));

        Some(centered_scrim_layer("confirm-dialog-layer", &theme, card))
    }

    // ── Permission run dialog ────────────────────────────────────────────

    pub(crate) fn render_permissions_run_dialog(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Some(run) = &self.permission_run else {
            return None;
        };
        let theme = Theme::current(cx);
        let fraction = if run.total == 0 {
            0.0
        } else {
            run.done as f32 / run.total as f32
        };
        let spinner = icon("icons/loader-circle.svg", 14.0, theme.text_tertiary)
            .with_animation(
                SharedString::from("permissions-run-spinner"),
                Animation::new(Duration::from_millis(900))
                    .repeat()
                    .with_easing(gpui::linear),
                |icon, delta| {
                    icon.with_transformation(gpui::Transformation::rotate(gpui::percentage(delta)))
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
                    .child(tr!(if run.grant {
                        "action.granting"
                    } else {
                        "action.revoking"
                    })),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.text_tertiary)
                    .child(SharedString::from(format!("{} / {}", run.done, run.total))),
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
                    .w(DefiniteLength::Fraction(fraction))
                    .rounded(px(4.0))
                    .bg(theme.accent),
            );
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .w_full()
                    .text_size(px(11.0))
                    .line_height(px(16.0))
                    .text_color(theme.text_secondary)
                    .child(tr!(
                        if run.grant { "action.grant_summary" } else { "action.revoke_summary" },
                        package = run.package,
                        total = run.total
                    )),
            )
            .child(bar);
        if !run.errors.is_empty() {
            body = body.child(
                div()
                    .w_full()
                    .text_size(px(11.0))
                    .text_color(theme.danger)
                    .child(tr!("action.failed_so_far", count = run.errors.len())),
            );
        }
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
            .child(title_row)
            .child(body);
        Some(centered_scrim_layer("permissions-run-layer", &theme, card))
    }

    // ── Status line ──────────────────────────────────────────────────────

    pub(crate) fn render_action_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::current(cx);
        let Some(status) = &self.app_action_status else {
            return div().into_any_element();
        };
        match status {
            AppActionStatus::Running { message, .. } => div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .min_w_0()
                .child(
                    icon("icons/loader-circle.svg", 12.0, theme.text_tertiary).with_animation(
                        SharedString::from("apps-action-status-spinner"),
                        Animation::new(Duration::from_millis(900))
                            .repeat()
                            .with_easing(gpui::linear),
                        |icon, delta| {
                            icon.with_transformation(gpui::Transformation::rotate(gpui::percentage(
                                delta,
                            )))
                        },
                    ),
                )
                .child(status_text(theme.text_tertiary, message))
                .into_any_element(),
            AppActionStatus::Done { message, .. } => div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .min_w_0()
                .child(icon("icons/check.svg", 11.0, theme.success))
                .child(status_text(theme.success, message))
                .into_any_element(),
            AppActionStatus::Error { message, .. } => div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .min_w_0()
                .child(icon("icons/x.svg", 11.0, theme.danger))
                .child(status_text(theme.danger, message))
                .into_any_element(),
        }
    }
}

fn status_text(color: gpui::Hsla, message: &str) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(px(11.0))
        .text_color(color)
        .child(SharedString::from(message))
}

fn context_menu_item(
    theme: &Theme,
    action: AppAction,
    package: SharedString,
    cx: &mut Context<Hakata>,
) -> Stateful<Div> {
    let destructive = action.destructive();
    let id = SharedString::from(format!(
        "package-context-{}",
        action.label().to_ascii_lowercase().replace(' ', "-")
    ));
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
        .text_color(if destructive {
            theme.danger
        } else {
            theme.text_secondary
        })
        .hover(|element| element.bg(theme.overlay))
        .active(|element| element.bg(theme.overlay_strong))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener({
                let package = package.clone();
                move |this, _, _, cx| {
                    this.dispatch_package_action(action, package.clone(), cx);
                    cx.stop_propagation();
                }
            }),
        )
        .child(action.label())
}

fn context_menu_more_row(theme: &Theme, open: bool, cx: &mut Context<Hakata>) -> Stateful<Div> {
    div()
        .id("package-context-more")
        .mx(px(4.0))
        .px(px(8.0))
        .min_h(px(26.0))
        .rounded(px(6.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_default()
        .hover(|element| element.bg(theme.overlay))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.toggle_context_more(cx);
                cx.stop_propagation();
            }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(11.5))
                .text_color(theme.text_secondary)
                .child(tr_cow!("action.more")),
        )
        .child(
            icon("icons/chevron-right.svg", 11.0, theme.text_tertiary).when(open, |icon| {
                icon.with_transformation(gpui::Transformation::rotate(gpui::percentage(0.25)))
            }),
        )
}

fn menu_divider(color: gpui::Hsla) -> Div {
    div().h(px(1.0)).mx(px(8.0)).my(px(4.0)).bg(color)
}

fn centered_scrim_layer(id: &'static str, theme: &Theme, card: Div) -> AnyElement {
    let scrim = if theme.is_dark {
        gpui::hsla(0.0, 0.0, 0.0, 0.34)
    } else {
        gpui::hsla(0.0, 0.0, 0.0, 0.16)
    };
    let layer = div()
        .id(id)
        .absolute()
        .inset_0()
        .occlude()
        .bg(scrim)
        .p(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .child(card);
    gpui::deferred(layer).with_priority(4).into_any_element()
}
