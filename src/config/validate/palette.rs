//! ANSI styling for the validation report.

use anstream::AutoStream;
use anstream::ColorChoice;
use anstyle::AnsiColor;
use anstyle::Style;

/// Report styling, mirroring cargo's colored `warning:` and `error:` prefixes. Detection follows
/// cargo's conventions: terminals are styled, `NO_COLOR` disables, `CLICOLOR_FORCE` styles even
/// non-terminal streams.
pub(super) struct Palette {
    /// Whether ANSI styling is enabled on stdout.
    enabled: bool,
}

impl Palette {
    /// Detects whether stdout accepts ANSI styling.
    pub(super) fn detect() -> Self {
        Self {
            enabled: AutoStream::choice(&std::io::stdout()) != ColorChoice::Never,
        }
    }

    /// Wraps the text in the given style.
    fn paint(&self, style: Style, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        format!("{}{text}{}", style.render(), style.render_reset())
    }

    pub(super) fn warning(&self, text: &str) -> String {
        self.paint(AnsiColor::Yellow.on_default().bold(), text)
    }

    pub(super) fn error(&self, text: &str) -> String {
        self.paint(AnsiColor::Red.on_default().bold(), text)
    }

    pub(super) fn valid(&self, text: &str) -> String {
        self.paint(AnsiColor::Green.on_default().bold(), text)
    }

    pub(super) fn heading(&self, text: &str) -> String {
        self.paint(Style::new().bold(), text)
    }

    pub(super) fn rule(&self) -> String {
        self.paint(Style::new().dimmed(), &"─".repeat(80))
    }
}
