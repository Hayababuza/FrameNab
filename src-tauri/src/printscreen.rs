//! Windows 11 opens its own screen capture (Snipping Tool) on the Print Screen key and takes the
//! key before any other program sees it. The settings page detects that and, only when the user
//! asks, hands the key back by switching that Windows option off for the current user.

use serde::Serialize;

/// Whether Windows takes the Print Screen key for its own screen capture.
#[cfg_attr(not(windows), allow(dead_code))] // only read on Windows
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PrintScreenOwner {
    /// Windows opens its screen capture; FrameNab does not get the key.
    Windows,
    /// The key is free for FrameNab.
    Free,
}

/// Decides from the stored option and the Windows build: Windows 11 (build 22000+) switched the
/// option on by default, so a missing value means "on" there and "off" on Windows 10.
#[cfg_attr(not(windows), allow(dead_code))]
fn owner(value: Option<u32>, build: Option<u32>) -> PrintScreenOwner {
    let enabled = value.map_or_else(|| build.is_some_and(|b| b >= 22000), |v| v != 0);
    if enabled { PrintScreenOwner::Windows } else { PrintScreenOwner::Free }
}

#[cfg(windows)]
mod platform {
    const KEYBOARD: &str = r"Control Panel\Keyboard";
    const VALUE: &str = "PrintScreenKeyForSnippingEnabled";

    pub fn value() -> Option<u32> {
        windows_registry::CURRENT_USER.open(KEYBOARD).ok()?.get_u32(VALUE).ok()
    }

    pub fn build() -> Option<u32> {
        let key = windows_registry::LOCAL_MACHINE.open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").ok()?;
        key.get_string("CurrentBuildNumber").ok()?.trim().parse().ok()
    }

    pub fn release() -> Result<(), String> {
        let key = windows_registry::CURRENT_USER.create(KEYBOARD).map_err(|e| e.to_string())?;
        key.set_u32(VALUE, 0).map_err(|e| e.to_string())
    }
}

/// Who gets the Print Screen key; `None` outside Windows.
#[tauri::command]
pub fn print_screen_owner() -> Option<PrintScreenOwner> {
    #[cfg(windows)]
    return Some(owner(platform::value(), platform::build()));
    #[cfg(not(windows))]
    None
}

/// Switches the Windows option off for the current user (no administrator rights needed). It takes
/// effect after signing out and in again.
#[tauri::command]
pub fn release_print_screen() -> Result<(), String> {
    #[cfg(windows)]
    {
        platform::release()?;
        tracing::info!("Print Screen key released from the Windows screen capture");
        Ok(())
    }
    #[cfg(not(windows))]
    Err("only available on Windows".into())
}

/// Opens the Windows keyboard settings, where the option can be switched back on.
#[tauri::command]
pub fn open_print_screen_settings(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url("ms-settings:easeofaccess-keyboard", None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_value_wins_and_default_depends_on_the_build() {
        assert_eq!(owner(Some(0), Some(26100)), PrintScreenOwner::Free);
        assert_eq!(owner(Some(1), Some(19045)), PrintScreenOwner::Windows);
        assert_eq!(owner(None, Some(22631)), PrintScreenOwner::Windows);
        assert_eq!(owner(None, Some(19045)), PrintScreenOwner::Free);
        assert_eq!(owner(None, None), PrintScreenOwner::Free);
    }
}
