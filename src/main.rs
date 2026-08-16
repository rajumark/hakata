use gpui::{
    App, Bounds, KeyBinding, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, actions, point, px, size,
};

rust_i18n::i18n!("locales", fallback = "en");

macro_rules! tr {
    ($key:expr) => {
        crate::i18n::translate($key)
    };
    ($key:expr, $($args:tt)*) => {
        rust_i18n::t!($key, $($args)*).into_owned()
    };
}

/// Borrow static translations on hot render paths; interpolation uses `tr!`
/// because formatted messages necessarily allocate.
macro_rules! tr_cow {
    ($key:literal) => {
        rust_i18n::t!($key)
    };
}

mod adb;
mod app;
mod app_icons;
mod assets;
mod emulator;
mod i18n;
mod input;
mod settings;
mod theme;

use app::Hakata;

const APP_NAME: &str = "Hakata";
const APP_ID: &str = "sh.hakata";

actions!(hakata, [Quit, CloseWindow, ToggleSidebar]);

fn main() {
    let loaded = settings::load();
    i18n::set_language(loaded.language);
    let theme_preference = loaded.theme;
    gpui_platform::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx: &mut App| {
            cx.set_app_identity(APP_ID, APP_NAME);
            theme::set_theme_preference(theme_preference, cx);
            input::init(cx);
            app::apps::init(cx);
            cx.on_action(|_: &Quit, cx| cx.quit());

            cx.bind_keys([
                KeyBinding::new("secondary-q", Quit, None),
                KeyBinding::new("secondary-w", CloseWindow, None),
                KeyBinding::new("secondary-b", ToggleSidebar, None),
            ]);

            #[cfg(not(target_os = "macos"))]
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1380.0), px(880.0)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        titlebar: Some(TitlebarOptions {
                            title: Some(APP_NAME.into()),
                            appears_transparent: true,
                            traffic_light_position: Some(point(px(16.0), px(17.0))),
                        }),
                        is_movable: true,
                        app_owns_titlebar_drag: cfg!(target_os = "macos"),
                        window_background: if cfg!(target_os = "macos") {
                            WindowBackgroundAppearance::Blurred
                        } else {
                            WindowBackgroundAppearance::Opaque
                        },
                        app_id: Some(APP_ID.to_owned()),
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        window_min_size: Some(size(px(980.0), px(680.0))),
                        ..Default::default()
                    },
                    |_window, cx| Hakata::new(cx),
                )
                .expect("failed to open Hakata window");

            cx.activate(true);

            cx.on_action({
                move |_: &ToggleSidebar, cx| {
                    window
                        .update(cx, |hakata, _window, cx| hakata.toggle_sidebar(cx))
                        .ok();
                }
            });
        });
}
