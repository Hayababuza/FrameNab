//! Tauri layer of FrameNab. Only glue code lives here (windows, tray, hotkeys, commands);
//! the actual logic is in `crates/*`.

mod cli;
mod commands;
mod hotkeys;
mod i18n;
mod logging;
mod tray;
mod windows;

use tauri::{Manager, WindowEvent};

pub use cli::Action;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial = cli::parse(std::env::args().skip(1));

    tauri::Builder::default()
        // Must be registered as the first plugin: a second launch (e.g. `framenab gui` from a
        // system shortcut) forwards its arguments to the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let action = cli::parse(argv.into_iter().skip(1)).action.unwrap_or(Action::Editor);
            windows::dispatch(app, action);
        }))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(hotkeys::plugin())
        .invoke_handler(tauri::generate_handler![commands::app_info, commands::run_action])
        .setup(move |app| {
            let log_dir = app.path().app_log_dir()?;
            app.manage(logging::init(&log_dir));
            tracing::info!(version = env!("CARGO_PKG_VERSION"), platform = %framenab_capture::Platform::detect(), "FrameNab starting");

            tray::create(app.handle())?;
            hotkeys::register_defaults(app.handle());

            if initial.hidden {
                windows::hide_main(app.handle());
            }
            if let Some(action) = initial.action {
                windows::dispatch(app.handle(), action);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window does not quit the app; it hides to the tray.
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == windows::MAIN
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to start FrameNab");
}
