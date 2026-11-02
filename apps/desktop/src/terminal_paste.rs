//! Which paste shortcut to send: Ctrl+V, or Ctrl+Shift+V for terminals
//! (Linux and Windows; macOS terminals take Cmd+V).

use std::sync::atomic::{AtomicU8, Ordering};

use crate::app_settings::TerminalPaste;

/// Application IDs, window classes, and executable stems of terminal
/// emulators, lowercase: Wayland app IDs (including reverse-DNS ones), X11
/// `WM_CLASS` names, and Windows executable stems.
const TERMINALS: &[&str] = &[
    "alacritty",
    "blackbox",
    "com.mitchellh.ghostty",
    "com.raggesilver.blackbox",
    "conhost",
    "contour",
    "cool-retro-term",
    "cmd",
    "deepin-terminal",
    "dev.warp.warp",
    "foot",
    "footclient",
    "ghostty",
    "gnome-terminal",
    "gnome-terminal-server",
    "guake",
    "io.elementary.terminal",
    "kgx",
    "kitty",
    "konsole",
    "lxterminal",
    "mate-terminal",
    "mintty",
    "openconsole",
    "org.contourterminal.contour",
    "org.gnome.console",
    "org.gnome.ptyxis",
    "org.gnome.terminal",
    "org.kde.konsole",
    "org.kde.yakuake",
    "org.wezfurlong.wezterm",
    "powershell",
    "ptyxis",
    "pwsh",
    "qterminal",
    "rio",
    "rxvt",
    "sakura",
    "st",
    "st-256color",
    "terminator",
    "tilix",
    "tabby",
    "urxvt",
    "uxterm",
    "warp",
    "wezterm",
    "wezterm-gui",
    "windowsterminal",
    "wt",
    "xfce4-terminal",
    "xterm",
    "yakuake",
];

/// Whether an application ID, window class, or executable stem names a
/// terminal emulator. Case-insensitive; an X11 `WM_CLASS` is two names
/// separated by a space, and either may match.
pub(crate) fn is_terminal(app_id: &str) -> bool {
    let app_id = app_id.to_lowercase();
    app_id
        .split_whitespace()
        .any(|name| TERMINALS.contains(&name))
}

/// Whether to send Ctrl+Shift+V. `focused_app` is only asked in Auto, and a
/// desktop that reports no focused window gets Ctrl+V.
pub(crate) fn paste_with_shift(
    setting: TerminalPaste,
    focused_app: impl FnOnce() -> Option<String>,
) -> bool {
    match setting {
        TerminalPaste::Always => true,
        TerminalPaste::Never => false,
        TerminalPaste::Auto => focused_app().is_some_and(|app| is_terminal(&app)),
    }
}

/// `TerminalPaste` in an atomic, shared live between the window and the
/// paster.
pub(crate) fn share(setting: TerminalPaste) -> std::sync::Arc<AtomicU8> {
    std::sync::Arc::new(AtomicU8::new(encode(setting)))
}

pub(crate) fn store(shared: &AtomicU8, setting: TerminalPaste) {
    shared.store(encode(setting), Ordering::Relaxed);
}

pub(crate) fn load(shared: &AtomicU8) -> TerminalPaste {
    match shared.load(Ordering::Relaxed) {
        1 => TerminalPaste::Always,
        2 => TerminalPaste::Never,
        _ => TerminalPaste::Auto,
    }
}

fn encode(setting: TerminalPaste) -> u8 {
    match setting {
        TerminalPaste::Auto => 0,
        TerminalPaste::Always => 1,
        TerminalPaste::Never => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_are_recognized_by_id_class_or_stem() {
        for terminal in [
            "Alacritty",
            "com.mitchellh.ghostty",
            "org.wezfurlong.wezterm",
            "org.gnome.Console",
            "org.gnome.Ptyxis",
            "kitty",
            "foot",
            "footclient",
            "org.kde.konsole",
            "XTerm",
            "Gnome-terminal",
            "xterm XTerm",
            "myterm Alacritty",
            "Xfce4-terminal",
            "Tilix",
            "terminator",
            "dev.warp.warp",
            "WindowsTerminal",
            "conhost",
            "pwsh",
        ] {
            assert!(is_terminal(terminal), "{terminal}");
        }
        for other in [
            "",
            "firefox",
            "org.mozilla.firefox",
            "code",
            "org.gnome.Nautilus",
            "slack",
            "terminal-emulator-settings",
            "notepad",
        ] {
            assert!(!is_terminal(other), "{other}");
        }
    }

    #[test]
    fn the_shortcut_follows_the_setting_and_the_focused_window() {
        let terminal = || Some("kitty".to_string());
        let other = || Some("firefox".to_string());
        assert!(paste_with_shift(TerminalPaste::Auto, terminal));
        assert!(!paste_with_shift(TerminalPaste::Auto, other));
        assert!(!paste_with_shift(TerminalPaste::Auto, || None));
        assert!(paste_with_shift(TerminalPaste::Always, other));
        assert!(!paste_with_shift(TerminalPaste::Never, terminal));
    }

    #[test]
    fn only_auto_looks_at_the_focused_window() {
        for setting in [TerminalPaste::Always, TerminalPaste::Never] {
            paste_with_shift(setting, || panic!("{setting:?} asked for the window"));
        }
    }

    #[test]
    fn the_shared_setting_round_trips() {
        let shared = share(TerminalPaste::Never);
        assert_eq!(load(&shared), TerminalPaste::Never);
        for setting in [
            TerminalPaste::Always,
            TerminalPaste::Auto,
            TerminalPaste::Never,
        ] {
            store(&shared, setting);
            assert_eq!(load(&shared), setting);
        }
    }
}
