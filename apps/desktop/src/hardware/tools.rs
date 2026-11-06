//! Other dictation tools already on this computer, so the first run's
//! shortcut step can say so: two tools listening for one shortcut both start
//! recording. [`observe`] reads the facts (a config file, the `PATH`);
//! [`assess`] is pure, so tests supply the facts.

use std::path::{Path, PathBuf};

/// A dictation tool the first run knows how to recognise.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OtherTool {
    /// Voxtype: its config file, or its executable on the `PATH`.
    Voxtype,
    /// WordInk's earlier Python client, `wordink-dictd`.
    WordinkDictd,
}

impl OtherTool {
    pub fn name(self) -> &'static str {
        match self {
            Self::Voxtype => "Voxtype",
            Self::WordinkDictd => "wordink-dictd",
        }
    }
}

/// What [`observe`] read.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Observed {
    /// `$XDG_CONFIG_HOME/voxtype/config.toml` (or `~/.config/...`) exists.
    pub voxtype_config: bool,
    /// The executables among [`EXECUTABLES`] found on the `PATH`.
    pub on_path: Vec<&'static str>,
}

/// The executable names looked for, without a Windows `.exe`.
pub const EXECUTABLES: [&str; 2] = ["voxtype", "wordink-dictd"];

/// The tools present, each once, in a stable order.
pub fn assess(observed: &Observed) -> Vec<OtherTool> {
    let mut tools = Vec::new();
    if observed.voxtype_config || observed.on_path.contains(&"voxtype") {
        tools.push(OtherTool::Voxtype);
    }
    if observed.on_path.contains(&"wordink-dictd") {
        tools.push(OtherTool::WordinkDictd);
    }
    tools
}

/// Reads this computer: cheap file checks only, nothing is run.
pub fn observe() -> Observed {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")));
    let voxtype_config =
        config_home.is_some_and(|base| base.join("voxtype").join("config.toml").is_file());
    let dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    Observed {
        voxtype_config,
        on_path: on_path(&dirs, |path| path.is_file()),
    }
}

/// The [`EXECUTABLES`] present in `dirs`, as `exists` sees them.
fn on_path(dirs: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Vec<&'static str> {
    EXECUTABLES
        .into_iter()
        .filter(|name| {
            dirs.iter()
                .any(|dir| exists(&dir.join(name)) || exists(&dir.join(format!("{name}.exe"))))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_found_is_no_tools() {
        assert!(assess(&Observed::default()).is_empty());
    }

    #[test]
    fn a_voxtype_config_or_executable_is_voxtype_once() {
        let config = Observed {
            voxtype_config: true,
            on_path: Vec::new(),
        };
        assert_eq!(assess(&config), [OtherTool::Voxtype]);
        let both = Observed {
            voxtype_config: true,
            on_path: vec!["voxtype"],
        };
        assert_eq!(assess(&both), [OtherTool::Voxtype]);
    }

    #[test]
    fn both_tools_are_listed_in_order() {
        let observed = Observed {
            voxtype_config: false,
            on_path: vec!["wordink-dictd", "voxtype"],
        };
        assert_eq!(
            assess(&observed),
            [OtherTool::Voxtype, OtherTool::WordinkDictd]
        );
    }

    #[test]
    fn executables_are_found_with_or_without_exe() {
        let dirs = [PathBuf::from("/opt/a"), PathBuf::from("/opt/b")];
        let present = ["/opt/b/voxtype", "/opt/a/wordink-dictd.exe"];
        let found = on_path(&dirs, |path| present.iter().any(|p| Path::new(p) == path));
        assert_eq!(found, ["voxtype", "wordink-dictd"]);
        assert!(on_path(&dirs, |_| false).is_empty());
    }
}
