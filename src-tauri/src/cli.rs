//! Command line, similar to `flameshot gui`. On Wayland apps may not register global shortcuts;
//! there you bind these commands to a key in the system settings.
//!
//! ```text
//! framenapper gui        open the screenshot overlay
//! framenapper record          choose the area to record (stops a running recording)
//! framenapper record-screen   record the whole screen (monitor under the mouse)
//! framenapper instant-gif     instant GIF: choose an area, records right away, saved and copied
//!                          when stopped (also `record-now`; stops a running recording)
//! framenapper editor     open the editor
//! framenapper settings   open the settings
//! framenapper edit <dir>      open a recording (*.framenapper folder) in the editor
//! framenapper --hidden   start in the tray only (autostart)
//! ```

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Screenshot,
    Record,
    RecordScreen,
    RecordNow,
    Editor,
    Settings,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub action: Option<Action>,
    pub hidden: bool,
    /// `edit <dir>`: project folder to open in the editor.
    pub project: Option<std::path::PathBuf>,
}

pub fn parse<I, S>(args: I) -> Args
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_ref() {
            "edit" | "open" => {
                out.action = Some(Action::Editor);
                out.project = args.next().map(|p| std::path::PathBuf::from(p.as_ref()));
            }
            "gui" | "screen" | "screenshot" => out.action = Some(Action::Screenshot),
            "record" => out.action = Some(Action::Record),
            "record-screen" => out.action = Some(Action::RecordScreen),
            "instant-gif" | "record-now" => out.action = Some(Action::RecordNow),
            "editor" => out.action = Some(Action::Editor),
            "settings" | "config" => out.action = Some(Action::Settings),
            "--hidden" | "--tray" => out.hidden = true,
            other => tracing::warn!(arg = other, "Ignoring unknown argument"),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subcommands() {
        assert_eq!(parse(["gui"]).action, Some(Action::Screenshot));
        assert_eq!(parse(["record-now"]).action, Some(Action::RecordNow));
        assert_eq!(parse(["instant-gif"]).action, Some(Action::RecordNow));
        assert_eq!(parse(["--hidden", "record"]), Args { action: Some(Action::Record), hidden: true, project: None });
        assert_eq!(parse(["edit", "/tmp/a.framenapper"]).project, Some(std::path::PathBuf::from("/tmp/a.framenapper")));
        assert_eq!(parse(Vec::<String>::new()), Args::default());
    }
}
