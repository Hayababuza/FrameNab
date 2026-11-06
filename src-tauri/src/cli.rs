//! Kommandozeile, analog zu `flameshot gui`. Unter Wayland dürfen Apps keine globalen
//! Tastenkürzel setzen; dort bindet man diese Befehle in den Systemeinstellungen an eine Taste.
//!
//! ```text
//! framenab gui        Screenshot-Overlay öffnen
//! framenab record     Aufnahme-Rahmen öffnen
//! framenab editor     Editor öffnen
//! framenab settings   Einstellungen öffnen
//! framenab --hidden   nur im Tray starten (Autostart)
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
            other => tracing::warn!(arg = other, "Unbekanntes Argument ignoriert"),
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
