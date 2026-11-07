//! Tauri layer of FrameNab. Only glue code lives here (windows, tray, hotkeys, commands);
//! the actual logic is in `crates/*`.

mod capture;
mod cli;
mod clipboard;
mod commands;
mod display;
mod editor;
mod hotkeys;
mod i18n;
mod logging;
mod pin;
mod recorder;
mod settings;
mod tray;
mod windows;

use tauri::{Manager, WindowEvent};

pub use cli::Action;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    display::choose_backend();
    let initial = cli::parse(std::env::args().skip(1));

    tauri::Builder::default()
        // Must be registered as the first plugin: a second launch (e.g. `framenab gui` from a
        // system shortcut) forwards its arguments to the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let args = cli::parse(argv.into_iter().skip(1));
            match args.project {
                Some(path) => open_project(app, std::path::Path::new(&cwd).join(path)),
                None => windows::dispatch(app, args.action.unwrap_or(Action::Editor)),
            }
        }))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(hotkeys::plugin())
        .register_asynchronous_uri_scheme_protocol("framenab", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || responder.respond(capture::protocol(&app, &request)));
        })
        .manage(capture::CaptureState::default())
        .manage(pin::PinState::default())
        .manage(recorder::RecorderState::default())
        .manage(editor::EditorState::default())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::run_action,
            capture::capture_info,
            capture::copy_selection,
            capture::save_selection,
            capture::pin_selection,
            capture::record_selection,
            capture::record_window,
            recorder::recorder_info,
            recorder::recorder_prepare,
            recorder::recorder_start,
            recorder::recorder_pause,
            recorder::recorder_status,
            recorder::recorder_stop,
            recorder::set_input_region,
            recorder::locate_marker,
            editor::editor_view,
            editor::editor_delete,
            editor::editor_undo,
            editor::editor_export,
            editor::editor_cancel_export,
            editor::recording_formats,
            editor::open_project,
            editor::recordings_directory,
            pin::pin_info,
            pin::copy_pin,
            settings::default_save_directory,
            capture::cancel_capture,
        ])
        .setup(move |app| {
            let log_dir = app.path().app_log_dir()?;
            app.manage(logging::init(&log_dir));
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                platform = %framenab_capture::Platform::detect(),
                xwayland = display::xwayland(),
                "FrameNab starting"
            );

            // Loading the system fonts takes a moment; do it now instead of on the first text.
            std::thread::spawn(framenab_render::text::warm_up);

            if let Some(main) = app.get_webview_window(windows::MAIN) {
                windows::fix_titlebar(&main);
            }
            tray::create(app.handle())?;
            hotkeys::register_defaults(app.handle());

            if initial.hidden {
                windows::hide_main(app.handle());
            }
            if let Some(path) = initial.project.clone() {
                open_project(app.handle(), path);
            } else if let Some(action) = initial.action {
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
            WindowEvent::Destroyed if window.label() == windows::RECORDER => recorder::cleanup(window.app_handle()),
            WindowEvent::Destroyed if window.label() == editor::LABEL => editor::cleanup(window.app_handle()),
            WindowEvent::Destroyed if window.label().starts_with(pin::PREFIX) => {
                pin::forget(window.app_handle(), window.label());
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("failed to start FrameNab");
}

/// Opens a recording folder in the editor, reporting failures in the log.
fn open_project(app: &tauri::AppHandle, path: std::path::PathBuf) {
    if let Err(err) = editor::open(app, path, None) {
        tracing::error!(%err, "could not open the project");
    }
}
