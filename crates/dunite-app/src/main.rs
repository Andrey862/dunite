//! Точка входа dunite (SPEC §6, roadmap M0).
//!
//! Использование: `dunite [путь-к-хранилищу]` (по умолчанию — текущая директория).

use std::path::PathBuf;

use dunite_settings::Settings;
use dunite_ui::Workspace;
use dunite_ui::actions::{CloseTab, NextTab, PrevTab, Quit};
use dunite_vault::Vault;
use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, SharedString, TitlebarOptions, WindowBounds,
    WindowOptions, px, size,
};
use tracing::{error, info};

fn main() {
    tracing_subscriber::fmt().init();

    let vault_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().expect("cwd"));

    let vault = match Vault::open(&vault_path) {
        Ok(vault) => vault,
        Err(err) => {
            error!(
                "не удалось открыть хранилище {}: {err}",
                vault_path.display()
            );
            std::process::exit(1);
        }
    };
    info!(
        root = %vault.root().display(),
        notes = vault.note_count(),
        "хранилище открыто"
    );

    let mut settings = Settings::load();
    settings.touch_vault(vault.root());
    if let Err(err) = settings.save() {
        error!("не удалось сохранить настройки: {err}");
    }

    let window_title: SharedString = format!("dunite — {}", vault.root().display()).into();

    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("ctrl-w", CloseTab, None),
            KeyBinding::new("ctrl-tab", NextTab, None),
            KeyBinding::new("ctrl-shift-tab", PrevTab, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(window_title),
                    ..Default::default()
                }),
                app_id: Some("dunite".to_string()),
                window_min_size: Some(size(px(800.), px(600.))),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let focus_handle = cx.focus_handle();
                    focus_handle.focus(window);
                    Workspace::new(vault, settings, focus_handle)
                })
            },
        )
        .expect("open window");
        cx.activate(true);
    });
}
