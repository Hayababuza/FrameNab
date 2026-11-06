//! Tauri layer of FrameNab. Only glue code lives here (windows, tray, hotkeys, commands);
//! the actual logic is in `crates/*`.

mod capture;
mod cli;
mod clipboard;
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
        .register_uri_scheme_protocol("framenab", capture::protocol)
        .manage(capture::CaptureState::default())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::run_action,
            capture::capture_info,
            capture::copy_selection,
            capture::cancel_capture,
        ])
        .setup(move |app| {
            let log_dir = app.path().app_log_dir()?;
            app.manage(logging::init(&log_dir));
            tracing::info!(version = env!("CARGO_PKG_VERSION"), platform = %framenab_capture::Platform::detect(), "FrameNab starting");

            // Loading the system fonts takes a moment; do it now instead of on the first text.
            std::thread::spawn(framenab_render::text::warm_up);

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
        .on_window_event(|window, event| match event {
            // Closing the main window does not quit the app; it hides to the tray.
            WindowEvent::CloseRequested { api, .. } if window.label() == windows::MAIN => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Destroyed if window.label() == windows::OVERLAY => capture::finish(window.app_handle()),
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("failed to start FrameNab");
}
