//! `wordink import voxtype`: brings a Voxtype `config.toml` across.
//!
//! Voxtype (https://github.com/peteonrails/voxtype, MIT) keeps its text
//! pipeline in `[text]` and its media handling in `[audio]`. The mapping to
//! WordInk's settings is WordInk's own; only the config key names come from
//! Voxtype's documented schema.
//!
//! | Voxtype key                       | WordInk setting                          |
//! |-----------------------------------|------------------------------------------|
//! | `text.replacements`               | Global mode replacements                 |
//! | `text.filter_fillers`             | `labs.filler_words`                      |
//! | `text.spoken_punctuation`         | `labs.spoken_formatting` + `spoken_symbols` |
//! | `audio.pause_media`               | `recording_audio_behavior = pause_media` |
//! | `audio.duck_media` (+ percent)    | `recording_audio_behavior = duck` (+ `recording_duck_volume`) |
//!
//! The merge only ever adds: a toggle Voxtype has off leaves WordInk's alone,
//! a replacement WordInk already has is kept, and a media behaviour the owner
//! already chose is not overwritten. So a second run changes nothing. The
//! summary prints counts and key names, never a replacement's text.

use std::path::{Path, PathBuf};

use color_eyre::eyre::{Result, WrapErr, eyre};

use crate::app_settings::{AppSettings, RecordingAudioBehavior, TextReplacement};

#[derive(clap::Subcommand)]
pub enum ImportCommand {
    /// Import a Voxtype config.toml: replacements, filler removal, spoken
    /// punctuation, and media pause or lowering. Safe to run again.
    Voxtype {
        /// The Voxtype config to read (default: $XDG_CONFIG_HOME/voxtype/config.toml).
        #[arg(long)]
        config: Option<PathBuf>,
        /// Show what would change and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

/// What a merge did. Counts and key names only.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub replacements_found: usize,
    pub replacements_added: usize,
    /// Already in WordInk with the same output.
    pub replacements_present: usize,
    /// Already in WordInk with another output; WordInk's is kept.
    pub replacements_conflicting: usize,
    pub fillers_enabled: bool,
    pub spoken_enabled: bool,
    pub media: Option<RecordingAudioBehavior>,
    /// Voxtype wanted media handling but WordInk already has its own choice.
    pub media_kept: bool,
    /// Keys with no WordInk equivalent, by dotted name.
    pub skipped_keys: Vec<String>,
    /// Mapped keys holding the wrong type of value.
    pub invalid_keys: Vec<String>,
}

impl Report {
    pub fn changed(&self) -> bool {
        self.replacements_added > 0
            || self.fillers_enabled
            || self.spoken_enabled
            || self.media.is_some()
    }
}

/// Applies a parsed Voxtype config to `settings`; no I/O.
pub fn merge(config: &toml::Table, settings: &mut AppSettings) -> Report {
    let mut report = Report::default();
    skipped_keys(config, &mut report);

    match config.get("text").and_then(toml::Value::as_table) {
        Some(text) => {
            merge_replacements(text, settings, &mut report);
            if let Some(on) = flag(text, "filter_fillers", "text", &mut report) {
                if on && !settings.labs.filler_words {
                    settings.labs.filler_words = true;
                    report.fillers_enabled = true;
                }
            }
            if let Some(on) = flag(text, "spoken_punctuation", "text", &mut report) {
                // Voxtype's one switch covers both of WordInk's: spoken
                // formatting ("new line") and spoken symbols ("comma").
                if on && !(settings.labs.spoken_formatting && settings.labs.spoken_symbols) {
                    settings.labs.spoken_formatting = true;
                    settings.labs.spoken_symbols = true;
                    report.spoken_enabled = true;
                }
            }
        }
        None if config.contains_key("text") => report.invalid_keys.push("text".into()),
        None => {}
    }

    if let Some(audio) = config.get("audio").and_then(toml::Value::as_table) {
        let pause = flag(audio, "pause_media", "audio", &mut report).unwrap_or(false);
        let duck = flag(audio, "duck_media", "audio", &mut report).unwrap_or(false);
        let percent = match audio.get("duck_media_volume_percent") {
            None => None,
            Some(value) => match value
                .as_integer()
                .filter(|percent| (0..=100).contains(percent))
            {
                Some(percent) => Some(percent as f32 / 100.0),
                None => {
                    report
                        .invalid_keys
                        .push("audio.duck_media_volume_percent".into());
                    None
                }
            },
        };
        // One behaviour at a time in WordInk; pausing wins over lowering.
        let wanted = if pause {
            Some(RecordingAudioBehavior::PauseMedia)
        } else if duck {
            Some(RecordingAudioBehavior::Duck)
        } else {
            None
        };
        if let Some(wanted) = wanted {
            let current = settings.recording_audio_behavior;
            if current == wanted {
                // Already there: nothing to do, and nothing was kept either.
            } else if current == RecordingAudioBehavior::DoNothing {
                settings.recording_audio_behavior = wanted;
                if wanted == RecordingAudioBehavior::Duck
                    && let Some(volume) = percent
                {
                    settings.recording_duck_volume = volume;
                }
                report.media = Some(wanted);
            } else {
                report.media_kept = true;
            }
        }
    }
    report.invalid_keys.sort();
    report
}

/// A boolean under `table[key]`; a value of another type is reported.
fn flag(table: &toml::Table, key: &str, section: &str, report: &mut Report) -> Option<bool> {
    let value = table.get(key)?;
    let flag = value.as_bool();
    if flag.is_none() {
        report.invalid_keys.push(format!("{section}.{key}"));
    }
    flag
}

fn merge_replacements(text: &toml::Table, settings: &mut AppSettings, report: &mut Report) {
    let Some(value) = text.get("replacements") else {
        return;
    };
    let Some(table) = value.as_table() else {
        report.invalid_keys.push("text.replacements".into());
        return;
    };
    let mut invalid = false;
    let rules = &mut settings.dictation_processing.default_mode.replacements;
    for (phrase, output) in table {
        let (Some(output), false) = (output.as_str(), phrase.trim().is_empty()) else {
            invalid = true;
            continue;
        };
        report.replacements_found += 1;
        let folded = phrase.trim().to_lowercase();
        match rules
            .iter()
            .find(|rule| rule.matched_phrase.trim().to_lowercase() == folded)
        {
            Some(existing) if existing.output == output => report.replacements_present += 1,
            Some(_) => report.replacements_conflicting += 1,
            None => {
                rules.push(TextReplacement {
                    matched_phrase: phrase.trim().to_string(),
                    output: output.to_string(),
                });
                report.replacements_added += 1;
            }
        }
    }
    if invalid {
        // The key is a spoken phrase, so only the table is named.
        report.invalid_keys.push("text.replacements.*".into());
    }
}

/// Keys WordInk has no setting for, by dotted name. Replacement phrases and
/// profile names are the owner's own words, so those tables are not opened.
fn skipped_keys(config: &toml::Table, report: &mut Report) {
    const MAPPED: &[&str] = &[
        "text.replacements",
        "text.filter_fillers",
        "text.spoken_punctuation",
        "audio.pause_media",
        "audio.duck_media",
        "audio.duck_media_volume_percent",
    ];
    const OPAQUE: &[&str] = &["profiles"];
    fn walk(table: &toml::Table, prefix: &str, out: &mut Vec<String>) {
        for (key, value) in table {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            if MAPPED.contains(&path.as_str()) {
                continue;
            }
            match value {
                toml::Value::Table(inner) if !OPAQUE.contains(&path.as_str()) => {
                    walk(inner, &path, out)
                }
                _ => out.push(path),
            }
        }
    }
    walk(config, "", &mut report.skipped_keys);
    report.skipped_keys.sort();
}

/// Reads the Voxtype config at `config` and merges it into the settings file
/// at `settings_path`. With `dry_run` nothing is written. A settings file is
/// rewritten only when the merge changed something.
pub fn import(config: &Path, settings_path: &Path, dry_run: bool) -> Result<Report> {
    let text = std::fs::read_to_string(config)
        .wrap_err_with(|| format!("could not read {}", config.display()))?;
    let table: toml::Table = text.parse().map_err(|error: toml::de::Error| {
        eyre!(
            "{} is not valid TOML: {}",
            config.display(),
            error.message()
        )
    })?;
    let (mut settings, _) = AppSettings::read_from(settings_path)?;
    let report = merge(&table, &mut settings);
    if !dry_run && report.changed() {
        settings.write_to(settings_path)?;
    }
    Ok(report)
}

/// The default Voxtype config: `$XDG_CONFIG_HOME/voxtype/config.toml`.
fn default_config_path() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| eyre!("cannot find the config folder; pass --config"))?;
    Ok(base.join("voxtype").join("config.toml"))
}

/// The lines `wordink import voxtype` prints: counts and key names only.
pub fn summary(report: &Report, dry_run: bool) -> String {
    let mut lines = vec![format!(
        "replacements: {} found, {} {}, {} already present, {} conflicting (kept yours)",
        report.replacements_found,
        report.replacements_added,
        if dry_run { "to add" } else { "added" },
        report.replacements_present,
        report.replacements_conflicting,
    )];
    let toggle = |name: &str, changed: bool| {
        format!(
            "{name}: {}",
            match (changed, dry_run) {
                (true, true) => "would turn on",
                (true, false) => "turned on",
                (false, _) => "no change",
            }
        )
    };
    lines.push(toggle("filler removal", report.fillers_enabled));
    lines.push(toggle("spoken punctuation", report.spoken_enabled));
    lines.push(match (report.media, report.media_kept) {
        (Some(behavior), _) => format!(
            "media: {} {}",
            if dry_run { "would set" } else { "set to" },
            behavior.label()
        ),
        (None, true) => "media: kept your existing choice".into(),
        (None, false) => "media: no change".into(),
    });
    if !report.invalid_keys.is_empty() {
        lines.push(format!(
            "invalid values skipped ({}): {}",
            report.invalid_keys.len(),
            report.invalid_keys.join(", ")
        ));
    }
    lines.push(format!(
        "skipped keys with no WordInk setting ({}){}{}",
        report.skipped_keys.len(),
        if report.skipped_keys.is_empty() {
            ""
        } else {
            ": "
        },
        report.skipped_keys.join(", ")
    ));
    if dry_run {
        lines.push("dry run: nothing was written".into());
    } else if report.changed() {
        lines.push("saved; restart WordInk if it is running to apply".into());
    } else {
        lines.push("nothing to change".into());
    }
    lines.join("\n")
}

pub fn run_cli(command: &ImportCommand) -> i32 {
    let ImportCommand::Voxtype { config, dry_run } = command;
    let result = (|| {
        let config = match config {
            Some(path) => path.clone(),
            None => default_config_path()?,
        };
        import(&config, &crate::app_settings::path()?, *dry_run)
    })();
    match result {
        Ok(report) => {
            println!("{}", summary(&report, *dry_run));
            0
        }
        Err(error) => {
            eprintln!("wordink: {error:#}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/voxtype/config.toml");

    fn table(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    fn global(settings: &AppSettings) -> Vec<(String, String)> {
        settings
            .dictation_processing
            .default_mode
            .replacements
            .iter()
            .map(|rule| (rule.matched_phrase.clone(), rule.output.clone()))
            .collect()
    }

    #[test]
    fn the_fixture_imports_into_the_expected_settings() {
        let mut settings = AppSettings::default();
        let report = merge(&table(FIXTURE), &mut settings);

        let mut rules = global(&settings);
        rules.sort();
        assert_eq!(
            rules,
            vec![
                ("acme corp".into(), "Acme Corp".into()),
                ("drop me".into(), String::new()),
                ("foo bar".into(), "foobar".into()),
                ("sample tool".into(), "SampleTool".into()),
            ]
        );
        assert!(settings.labs.filler_words);
        assert!(settings.labs.spoken_formatting);
        assert!(settings.labs.spoken_symbols);
        assert_eq!(
            settings.recording_audio_behavior,
            RecordingAudioBehavior::PauseMedia
        );
        assert_eq!(report.replacements_found, 4);
        assert_eq!(report.replacements_added, 4);
        assert!(report.fillers_enabled && report.spoken_enabled);
        assert_eq!(report.media, Some(RecordingAudioBehavior::PauseMedia));
    }

    #[test]
    fn importing_again_adds_nothing() {
        let mut settings = AppSettings::default();
        merge(&table(FIXTURE), &mut settings);
        let after_first = settings.clone();
        let report = merge(&table(FIXTURE), &mut settings);
        assert_eq!(settings, after_first);
        assert_eq!(report.replacements_added, 0);
        assert_eq!(report.replacements_present, 4);
        assert!(!report.changed());
    }

    #[test]
    fn an_existing_replacement_wins_and_case_does_not_duplicate() {
        let mut settings = AppSettings::default();
        settings.dictation_processing.default_mode.replacements = vec![TextReplacement {
            matched_phrase: "Acme Corp".into(),
            output: "ACME".into(),
        }];
        let report = merge(&table(FIXTURE), &mut settings);
        assert_eq!(global(&settings).len(), 4);
        assert!(global(&settings).contains(&("Acme Corp".into(), "ACME".into())));
        assert_eq!(report.replacements_conflicting, 1);
        assert_eq!(report.replacements_added, 3);
    }

    #[test]
    fn a_toggle_voxtype_has_off_leaves_wordink_alone() {
        let mut settings = AppSettings::default();
        settings.labs.filler_words = true;
        let report = merge(
            &table("[text]\nfilter_fillers = false\nspoken_punctuation = false\n"),
            &mut settings,
        );
        assert!(settings.labs.filler_words);
        assert!(!settings.labs.spoken_formatting);
        assert!(!report.changed());
    }

    #[test]
    fn ducking_maps_with_its_volume_and_keeps_an_existing_choice() {
        let config = table("[audio]\nduck_media = true\nduck_media_volume_percent = 35\n");
        let mut settings = AppSettings::default();
        let report = merge(&config, &mut settings);
        assert_eq!(report.media, Some(RecordingAudioBehavior::Duck));
        assert_eq!(
            settings.recording_audio_behavior,
            RecordingAudioBehavior::Duck
        );
        assert!((settings.recording_duck_volume - 0.35).abs() < 1e-6);

        let mut chosen = AppSettings::default();
        chosen.recording_audio_behavior = RecordingAudioBehavior::Mute;
        let report = merge(&config, &mut chosen);
        assert!(report.media_kept && report.media.is_none());
        assert_eq!(
            chosen.recording_audio_behavior,
            RecordingAudioBehavior::Mute
        );
    }

    #[test]
    fn unknown_and_unmapped_keys_are_reported_and_skipped() {
        let mut settings = AppSettings::default();
        let report = merge(
            &table(
                "[text]\nfuture_toggle = true\n[text.replacements]\n\"secret phrase\" = \"x\"\n\
                 [vad]\nenabled = true\n[profiles.mine]\nmode = \"x\"\n",
            ),
            &mut settings,
        );
        assert_eq!(
            report.skipped_keys,
            vec!["profiles", "text.future_toggle", "vad.enabled"]
        );
        let shown = format!("{report:?} {}", super::summary(&report, false));
        assert!(!shown.contains("secret phrase"));
    }

    #[test]
    fn wrongly_typed_mapped_keys_are_reported() {
        let mut settings = AppSettings::default();
        let report = merge(
            &table("[text]\nfilter_fillers = \"yes\"\n[text.replacements]\nok = 3\n"),
            &mut settings,
        );
        assert_eq!(
            report.invalid_keys,
            vec!["text.filter_fillers", "text.replacements.*"]
        );
        assert!(!settings.labs.filler_words);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wordink-voxtype-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn dry_run_writes_nothing_and_a_real_run_persists_once() {
        let dir = scratch("files");
        let config = dir.join("config.toml");
        let settings = dir.join("settings.json");
        std::fs::write(&config, FIXTURE).unwrap();

        let report = import(&config, &settings, true).unwrap();
        assert_eq!(report.replacements_added, 4);
        assert!(!settings.exists(), "dry run created settings.json");

        import(&config, &settings, false).unwrap();
        let saved = AppSettings::read_from(&settings).unwrap().0;
        assert_eq!(global(&saved).len(), 4);
        assert!(saved.labs.filler_words);

        let bytes = std::fs::read(&settings).unwrap();
        let again = import(&config, &settings, false).unwrap();
        assert!(!again.changed());
        assert_eq!(std::fs::read(&settings).unwrap(), bytes);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_broken_config_is_an_error_that_writes_nothing() {
        let dir = scratch("errors");
        let settings = dir.join("settings.json");
        assert!(import(&dir.join("none.toml"), &settings, false).is_err());
        std::fs::write(dir.join("bad.toml"), "[text\n").unwrap();
        assert!(import(&dir.join("bad.toml"), &settings, false).is_err());
        assert!(!settings.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
