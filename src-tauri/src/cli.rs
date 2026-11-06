//! Command line, similar to `flameshot gui`. On Wayland apps may not register global shortcuts;
//! there you bind these commands to a key in the system settings.
//!
//! ```text
//! framenab gui        open the screenshot overlay
//! framenab record     open the recording frame
//! framenab editor     open the editor
//! framenab settings   open the settings
//! framenab --hidden   start in the tray only (autostart)
//! ```

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Screenshot,
    Record,
    Editor,
    Settings,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub action: Option<Action>,
    pub hidden: bool,
}

pub fn parse<I, S>(args: I) -> Args
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = Args::default();
    for arg in args {
        match arg.as_ref() {
            "gui" | "screen" | "screenshot" => out.action = Some(Action::Screenshot),
            "record" => out.action = Some(Action::Record),
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
        assert_eq!(parse(["--hidden", "record"]), Args { action: Some(Action::Record), hidden: true });
        assert_eq!(parse(Vec::<String>::new()), Args::default());
    }
}
