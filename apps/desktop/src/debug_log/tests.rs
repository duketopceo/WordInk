use super::*;
use serde_json::{Value, json};

const KEY: &str = "gsk_window_secret_0123456789";

fn event(level: DebugLevel, stage: &str, message: &str) -> Event {
    let (subsystem, name) = stage.split_once('.').unwrap_or((stage, ""));
    Event {
        seq: 0,
        mono_ms: 4821,
        wall_ms: 1_700_000_000_123,
        level,
        subsystem: subsystem.into(),
        name: name.into(),
        message: message.into(),
        fields: Vec::new(),
    }
}

fn with_fields(mut event: Event, fields: &[(&str, Value)]) -> Event {
    event.fields = fields
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect();
    event
}

fn redactions() -> Redactions {
    Redactions {
        home: Some("/home/kit".into()),
        user: Some("kit".into()),
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "wordink-debug-log-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn timestamps_are_utc_rfc3339_with_milliseconds() {
    assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(rfc3339(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
    // A leap day.
    assert_eq!(rfc3339(1_709_164_800_000), "2024-02-29T00:00:00.000Z");
}

#[test]
fn transcripts_are_withheld_unless_opted_in() {
    // Tests never switch the global log on, so the default applies.
    assert_eq!(transcript("my private words"), "<withheld 16 chars>");
}

#[test]
fn the_log_rotates_at_its_cap_and_keeps_three_files() {
    let dir = temp_dir("rotate");
    let max = 400;
    let mut sink = Sink::open(&dir, max, KEEP_FILES).unwrap();
    for n in 0..200 {
        sink.write_line(&format!("line {n:04} {}", "x".repeat(40)));
    }
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["debug.log", "debug.log.1", "debug.log.2"]);
    let mut total = 0;
    for name in &names {
        let size = std::fs::metadata(dir.join(name)).unwrap().len();
        assert!(size <= max, "{name} is {size} bytes");
        total += size;
    }
    assert!(total <= 3 * max);
    // The newest line is in the live file; the oldest lines are gone.
    let live = std::fs::read_to_string(dir.join("debug.log")).unwrap();
    assert!(live.contains("line 0199"));
    let kept: String = names
        .iter()
        .map(|name| std::fs::read_to_string(dir.join(name)).unwrap())
        .collect();
    assert!(!kept.contains("line 0000"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rotation_with_one_file_kept_starts_the_file_over() {
    let dir = temp_dir("single");
    let mut sink = Sink::open(&dir, 100, 1).unwrap();
    for n in 0..20 {
        sink.write_line(&format!("line {n:04} {}", "y".repeat(30)));
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    assert!(std::fs::metadata(dir.join("debug.log")).unwrap().len() <= 100);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn clearing_removes_every_debug_log_file_and_nothing_else() {
    let dir = temp_dir("clear");
    for name in ["debug.log", "debug.log.1", "debug.log.2", "live.ndjson"] {
        std::fs::write(dir.join(name), "x\n").unwrap();
    }
    assert_eq!(clear_in(&dir), 3);
    assert!(dir.join("live.ndjson").exists());
    assert!(!dir.join("debug.log").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn values_with_spaces_or_quotes_are_quoted_and_escaped() {
    assert_eq!(quoted("plain"), "plain");
    assert_eq!(quoted("two words"), "\"two words\"");
    assert_eq!(quoted("say \"hi\""), "\"say \\\"hi\\\"\"");
    assert_eq!(quoted(""), "\"\"");
    assert_eq!(json!(3).to_string(), "3");
}

#[test]
fn a_line_names_wall_and_monotonic_time_level_event_and_fields() {
    let line = format_text(
        &with_fields(
            event(DebugLevel::Debug, "audio.capture", "capture summary"),
            &[
                ("device", val("bluez_input.AA_BB.0")),
                ("label", val("Between 3ANC")),
                ("rms", val(&0.0)),
                ("silent", val(&true)),
                ("ms", val(&37)),
            ],
        ),
        &redactions(),
    );
    assert_eq!(
        line,
        "2023-11-14T22:13:20.123Z +4821ms DEBUG audio.capture capture summary \
         device=bluez_input.AA_BB.0 label=\"Between 3ANC\" rms=0.0 silent=true ms=37"
    );
}

#[test]
fn json_lines_carry_the_same_event_for_machines() {
    let json = format_json(
        &with_fields(
            event(DebugLevel::Info, "engine.request", "sent"),
            &[("status", val(&200)), ("route", val("local"))],
        ),
        &redactions(),
    );
    let parsed: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["level"], "info");
    assert_eq!(parsed["subsystem"], "engine");
    assert_eq!(parsed["event"], "request");
    assert_eq!(parsed["mono_ms"], 4821);
    assert_eq!(parsed["wall_ms"], 1_700_000_000_123u64);
    assert_eq!(parsed["fields"]["status"], 200);
    assert_eq!(parsed["fields"]["route"], "local");
    assert!(!json.contains('\n'));
}

#[test]
fn nothing_renders_a_key_or_this_machines_home_folder_and_user() {
    let e = with_fields(
        event(DebugLevel::Info, "engine.request", "sent"),
        &[
            ("token", val(KEY)),
            (
                "detail",
                val("failed reading /home/kit/.config/wordink/x for kit"),
            ),
        ],
    );
    for line in [
        format_text(&e, &redactions()),
        format_json(&e, &redactions()),
    ] {
        assert!(!line.contains(KEY), "{line}");
        assert!(!line.contains("/home/kit"), "{line}");
        assert!(!line.contains("for kit"), "{line}");
    }
}

#[test]
fn copied_text_is_scrubbed_of_paths_keys_and_the_user() {
    let events = [
        with_fields(
            event(
                DebugLevel::Info,
                "output.paste",
                "opened /home/kit/.local/share/x",
            ),
            &[("key", val(KEY))],
        ),
        event(DebugLevel::Info, "output.paste", "user kit said hi"),
    ];
    let text = copy_text(&events, &redactions());
    assert_eq!(text.lines().count(), 2);
    for secret in ["/home/kit", KEY, " kit "] {
        assert!(!text.contains(secret), "a fixture secret survived the copy");
    }
}

fn ring_events(count: usize, message: &str) -> Ring {
    let mut ring = Ring::new(10, usize::MAX);
    for n in 0..count {
        ring.push(event(
            DebugLevel::Info,
            "audio.x",
            &format!("{message} {n}"),
        ));
    }
    ring
}

#[test]
fn the_ring_keeps_only_the_newest_events_past_its_count_bound() {
    let ring = ring_events(25, "event");
    assert_eq!(ring.len(), 10);
    let (events, next) = ring.since(0, &Filter::default(), 100);
    assert_eq!(events.first().unwrap().message, "event 15");
    assert_eq!(events.last().unwrap().message, "event 24");
    assert_eq!(
        events.last().unwrap().seq,
        25,
        "sequence numbers keep counting"
    );
    assert_eq!(next, 25);
}

#[test]
fn the_ring_evicts_oldest_events_past_its_byte_bound() {
    let mut ring = Ring::new(usize::MAX, 4_000);
    for n in 0..200 {
        ring.push(event(
            DebugLevel::Info,
            "audio.x",
            &format!("{n} {}", "z".repeat(100)),
        ));
    }
    assert!(ring.bytes() <= 4_000, "{} bytes", ring.bytes());
    assert!(ring.len() > 1 && ring.len() < 200);
    let (events, _) = ring.since(0, &Filter::default(), 1000);
    assert!(events.last().unwrap().message.starts_with("199 "));
    assert!(!events.iter().any(|e| e.message.starts_with("0 ")));
    // One event bigger than the bound is still kept: the newest is never lost.
    let mut tiny = Ring::new(usize::MAX, 10);
    tiny.push(event(DebugLevel::Info, "audio.x", "big event"));
    assert_eq!(tiny.len(), 1);
}

#[test]
fn the_default_bounds_are_five_thousand_events_and_two_megabytes() {
    assert_eq!(RING_EVENTS, 5_000);
    assert_eq!(RING_BYTES, 2 * 1024 * 1024);
}

#[test]
fn filters_by_level_subsystem_and_search() {
    let events = [
        event(DebugLevel::Info, "audio.route", "moved to the earbuds"),
        event(DebugLevel::Debug, "audio.inventory", "devices listed"),
        event(DebugLevel::Trace, "audio.chunk", "chunk rms"),
        event(DebugLevel::Info, "bluetooth.restore", "profile restored"),
        event(DebugLevel::Info, "engine.request", "sent"),
    ];
    let shown = |filter: &Filter| {
        events
            .iter()
            .filter(|e| filter.matches(e))
            .map(|e| e.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(shown(&Filter::default()).len(), 5);
    let info = Filter {
        level: DebugLevel::Info,
        ..Filter::default()
    };
    assert_eq!(shown(&info), ["route", "restore", "request"]);
    let debug_audio = Filter {
        level: DebugLevel::Debug,
        subsystems: vec!["audio".into()],
        search: String::new(),
    };
    assert_eq!(shown(&debug_audio), ["route", "inventory"]);
    let search = Filter {
        search: "EARBUDS".into(),
        ..Filter::default()
    };
    assert_eq!(shown(&search), ["route"]);
    let two = Filter {
        subsystems: vec!["bluetooth".into(), "engine".into()],
        ..Filter::default()
    };
    assert_eq!(shown(&two), ["restore", "request"]);
}

#[test]
fn a_follow_resumes_from_its_cursor_without_gaps_or_repeats() {
    let mut ring = ring_events(4, "a");
    let filter = Filter::default();
    let (first, cursor) = ring.since(0, &filter, 100);
    assert_eq!(first.len(), 4);
    assert!(ring.since(cursor, &filter, 100).0.is_empty());
    ring.push(event(DebugLevel::Info, "audio.x", "b 0"));
    ring.push(event(DebugLevel::Info, "audio.x", "b 1"));
    let (next, cursor2) = ring.since(cursor, &filter, 100);
    assert_eq!(
        next.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(),
        ["b 0", "b 1"]
    );
    // A small limit pages: the cursor stops at the last event returned.
    ring.push(event(DebugLevel::Info, "audio.x", "c 0"));
    ring.push(event(DebugLevel::Info, "audio.x", "c 1"));
    let (page, cursor3) = ring.since(cursor2, &filter, 1);
    assert_eq!(page[0].message, "c 0");
    let (rest, _) = ring.since(cursor3, &filter, 1);
    assert_eq!(rest[0].message, "c 1");
}

#[test]
fn a_filtered_follow_skips_hidden_events_but_still_advances() {
    let mut ring = Ring::new(10, usize::MAX);
    ring.push(event(DebugLevel::Debug, "audio.x", "hidden"));
    let info = Filter {
        level: DebugLevel::Info,
        ..Filter::default()
    };
    let (events, cursor) = ring.since(0, &info, 10);
    assert!(events.is_empty());
    assert_eq!(cursor, 1, "the hidden event is not asked for again");
}

#[test]
fn history_returns_the_newest_events_when_it_exceeds_the_limit() {
    let ring = ring_events(10, "e");
    let (events, cursor) = ring.since(0, &Filter::default(), 3);
    assert_eq!(
        events
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>(),
        ["e 7", "e 8", "e 9"]
    );
    assert_eq!(cursor, 10);
}

#[test]
fn a_subscriber_gets_the_buffered_history_then_live_events_with_no_overlap() {
    let mut ring = ring_events(3, "old");
    let (history, live) = ring.subscribe();
    assert_eq!(history.len(), 3);
    ring.push(event(DebugLevel::Info, "audio.x", "new 0"));
    ring.push(event(DebugLevel::Info, "audio.x", "new 1"));
    let received: Vec<_> = live.try_iter().collect();
    assert_eq!(
        received
            .iter()
            .map(|e| e.message.as_str())
            .collect::<Vec<_>>(),
        ["new 0", "new 1"]
    );
    assert_eq!(received[0].seq, history.last().unwrap().seq + 1);
    // A dropped receiver is forgotten, not an error.
    drop(live);
    ring.push(event(DebugLevel::Info, "audio.x", "after"));
    assert_eq!(ring.len(), 6);
}

#[test]
fn modules_are_filed_under_their_subsystem() {
    assert_eq!(subsystem_of("hex::linux::headset"), "bluetooth");
    assert_eq!(subsystem_of("hex::audio::bluetooth_mic"), "bluetooth");
    assert_eq!(subsystem_of("hex::linux::pulse"), "audio");
    assert_eq!(subsystem_of("hex::transcription::cloud"), "engine");
    assert_eq!(subsystem_of("hex::linux::paste"), "output");
    assert_eq!(subsystem_of("hex::linux::audio_control"), "media");
    assert_eq!(subsystem_of("hex::updater::linux"), "updater");
    assert_eq!(subsystem_of("hex::something_else"), "app");
}

#[test]
fn the_levels_parse_and_order() {
    assert_eq!(DebugLevel::parse(" Debug "), Some(DebugLevel::Debug));
    assert_eq!(DebugLevel::parse("loud"), None);
    assert!(DebugLevel::Info < DebugLevel::Debug && DebugLevel::Debug < DebugLevel::Trace);
}

#[test]
fn the_bridge_wants_this_binarys_own_events() {
    // Targets begin with the binary's crate name, not the package name "hex".
    assert_eq!(crate::app_paths::crate_root(), "wordink");
    assert!(module_path!().starts_with(crate::app_paths::crate_root()));
    assert_eq!(
        module_of(module_path!()).split("::").next(),
        Some("debug_log")
    );
    assert_eq!(module_of("wordink::windows::dictation"), "windows::dictation");
    assert_eq!(module_of("hex::linux::paste"), "linux::paste");
}
