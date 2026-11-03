use super::*;

/// An engine that answers what it is told, instantly and offline.
struct Fake {
    spec: EngineSpec,
    answer: Result<Outcome, String>,
}

fn fake(label: &str, answer: Result<Outcome, String>) -> Box<dyn Engine> {
    Box::new(Fake {
        spec: EngineSpec::parse(label).unwrap(),
        answer,
    })
}

fn said(text: &str, latency_ms: u64) -> Result<Outcome, String> {
    Ok(Outcome {
        text: text.into(),
        latency_ms: Some(latency_ms),
        ..Outcome::default()
    })
}

impl Engine for Fake {
    fn spec(&self) -> &EngineSpec {
        &self.spec
    }

    fn transcribe(&mut self, _: &[f32]) -> Result<Outcome, String> {
        self.answer.clone()
    }
}

fn words(text: &str) -> Vec<String> {
    normalize_words(text)
}

fn row(engine: &str, ok: bool, latency_ms: u64, text: &str, cost: Option<f64>) -> Row {
    Row {
        ts_ms: 1,
        clip: "c1".into(),
        audio_seconds: 10.0,
        engine: engine.into(),
        ok,
        error: (!ok).then(|| "boom".into()),
        latency_ms,
        rtf: latency_ms as f64 / 10_000.0,
        request_bytes: None,
        encode_ms: None,
        cost_usd: cost,
        words: text.split_whitespace().count(),
        device: None,
        wer: None,
        text: text.into(),
    }
}

#[test]
fn engines_are_named_provider_colon_model_and_a_bare_provider_means_its_default() {
    let spec = EngineSpec::parse("openrouter:microsoft/mai-transcribe-2").unwrap();
    assert_eq!(
        (spec.provider.as_str(), spec.model.as_str()),
        ("openrouter", "microsoft/mai-transcribe-2")
    );
    assert_eq!(spec.label(), "openrouter:microsoft/mai-transcribe-2");
    assert_eq!(
        EngineSpec::parse("groq").unwrap().label(),
        "groq:whisper-large-v3-turbo"
    );
    assert!(EngineSpec::parse("local:parakeet-v3").unwrap().is_local());
    assert_eq!(EngineSpec::parse("local").unwrap().label(), "local");
    assert!(EngineSpec::parse("bing:x").is_err());
    let list = EngineSpec::parse_list(
        "groq:whisper-large-v3-turbo, groq:whisper-large-v3,local:parakeet-v3",
    )
    .unwrap();
    assert_eq!(list.len(), 3);
    assert!(EngineSpec::parse_list(" , ").is_err());
}

#[test]
fn a_run_keeps_the_order_of_the_engines_and_a_failure_does_not_stop_the_rest() {
    let mut engines = vec![
        fake("groq:whisper-large-v3-turbo", said("hello world", 900)),
        fake("openai:whisper-1", Err("OpenAI key rejected".into())),
        fake(
            "local:parakeet-v3",
            Ok(Outcome {
                text: "hello word".into(),
                latency_ms: Some(2_500),
                cost_usd: Some(0.0),
                device: Some("Test GPU (vulkan)".into()),
                ..Outcome::default()
            }),
        ),
    ];
    // Ten seconds of audio.
    let samples = vec![0.1_f32; 160_000];
    let rows = run(&mut engines, &samples, None);
    let names: Vec<&str> = rows.iter().map(|row| row.engine.as_str()).collect();
    assert_eq!(
        names,
        [
            "groq:whisper-large-v3-turbo",
            "openai:whisper-1",
            "local:parakeet-v3"
        ]
    );
    assert!(rows[0].ok && !rows[1].ok && rows[2].ok);
    assert_eq!(rows[1].error.as_deref(), Some("OpenAI key rejected"));
    assert!((rows[0].audio_seconds - 10.0).abs() < 1e-9);
    assert!((rows[0].rtf - 0.09).abs() < 1e-9, "900 ms over 10 s");
    assert!((rows[2].rtf - 0.25).abs() < 1e-9);
    assert_eq!(rows[2].device.as_deref(), Some("Test GPU (vulkan)"));
    assert_eq!(rows[0].words, 2);
    assert!(rows.iter().all(|row| row.clip == rows[0].clip));
    assert_eq!(rows[0].clip, clip_id(&samples));
    assert_ne!(clip_id(&samples), clip_id(&vec![0.2_f32; 160_000]));
}

#[test]
fn with_a_reference_every_row_carries_its_word_error_rate() {
    let mut engines = vec![
        fake("groq", said("The quick brown fox.", 1)),
        fake("openai", said("the quick brown box jumps", 1)),
    ];
    let rows = run(&mut engines, &[0.0; 16_000], Some("the quick brown fox"));
    assert_eq!(rows[0].wer, Some(0.0), "case and punctuation do not count");
    // One substitution and one insertion over four reference words.
    assert_eq!(rows[1].wer, Some(0.5));
}

#[test]
fn the_word_diff_marks_what_is_missing_added_and_changed() {
    let reference = words("the quick brown fox jumps");
    let heard = words("the quick red fox jumps over");
    let (ops, edits) = align(&reference, &heard);
    assert_eq!(edits, 2);
    assert_eq!(
        render_diff(&ops),
        "the quick [-brown-]{+red+} fox jumps {+over+}"
    );
    let (ops, _) = align(&words("a b c"), &words("a c"));
    assert_eq!(render_diff(&ops), "a [-b-] c");
    assert_eq!(diff_text("same words", "Same, words!"), "same words");
}

#[test]
fn word_error_rate_is_edits_over_the_reference_length() {
    assert_eq!(wer("one two three four", "one two three four"), 0.0);
    assert_eq!(wer("one two three four", "one too three"), 0.5);
    assert_eq!(wer("one two", ""), 1.0);
    assert_eq!(wer("", ""), 0.0);
    assert_eq!(wer("", "extra"), 1.0);
    // An apostrophe stays inside a word; other punctuation goes.
    assert_eq!(wer("don't stop", "Don't, stop."), 0.0);
}

#[test]
fn side_by_side_wraps_each_text_in_its_own_column() {
    let text = side_by_side(
        &[
            ("groq".into(), "one two three four five six".into()),
            ("local".into(), "uno".into()),
        ],
        27,
    );
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("groq"));
    assert!(lines[0].contains(" | local"));
    assert!(lines.len() > 3, "the long text wraps: {text}");
    assert!(lines.iter().all(|line| line.chars().count() <= 27));
    assert_eq!(side_by_side(&[], 80), "");
}

#[test]
fn the_ranking_orders_by_word_error_rate_then_latency_and_puts_failures_last() {
    let mut rows = Vec::new();
    // Three runs each, so the median is the middle one.
    for latency in [1_000, 1_200, 5_000] {
        rows.push(row(
            "groq:turbo",
            true,
            latency,
            "the quick brown fox",
            Some(0.001),
        ));
    }
    for latency in [300, 400, 500] {
        rows.push(row(
            "openai:mini",
            true,
            latency,
            "the quick brown box",
            Some(0.003),
        ));
    }
    for latency in [2_000, 2_100, 2_200] {
        rows.push(row(
            "local:parakeet",
            true,
            latency,
            "the quick brown fox",
            Some(0.0),
        ));
    }
    rows.push(row("deepgram:nova-3", false, 0, "", None));
    let ranked = rank(&rows, Some("the quick brown fox"), None);
    let order: Vec<&str> = ranked.iter().map(|entry| entry.engine.as_str()).collect();
    // Both exact engines first (the faster of them first), then the one
    // with an error, then the engine that never answered.
    assert_eq!(
        order,
        [
            "groq:turbo",
            "local:parakeet",
            "openai:mini",
            "deepgram:nova-3"
        ]
    );
    assert_eq!(
        ranked.iter().map(|entry| entry.rank).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    let groq = &ranked[0];
    assert_eq!((groq.runs, groq.failures), (3, 0));
    assert_eq!(groq.median_latency_ms, Some(1_200));
    assert_eq!(groq.wer, Some(0.0));
    assert!((ranked[2].wer.unwrap() - 0.25).abs() < 1e-9);
    // Dollars per hour of audio: 0.003 for each 10 s is 1.08 an hour.
    assert!((ranked[2].cost_per_hour.unwrap() - 1.08).abs() < 1e-9);
    assert_eq!(ranked[1].cost_per_hour, Some(0.0));
    let dead = &ranked[3];
    assert_eq!((dead.runs, dead.failures), (1, 1));
    assert_eq!(
        (dead.median_latency_ms, dead.wer, dead.cost_per_hour),
        (None, None, None)
    );
}

#[test]
fn without_a_reference_the_ranking_is_by_median_latency() {
    let rows = vec![
        row("slow", true, 900, "a", None),
        row("fast", true, 100, "b", None),
        row("mid", true, 400, "c", None),
    ];
    let ranked = rank(&rows, None, None);
    let order: Vec<&str> = ranked.iter().map(|entry| entry.engine.as_str()).collect();
    assert_eq!(order, ["fast", "mid", "slow"]);
    assert!(ranked.iter().all(|entry| entry.wer.is_none()));
    // A clip filter keeps only that clip's rows.
    let mut other = row("fast", true, 50, "d", None);
    other.clip = "c2".into();
    let mut rows = rows;
    rows.push(other);
    assert_eq!(rank(&rows, None, Some("c2")).len(), 1);
}

#[test]
fn a_rows_own_word_error_rate_beats_the_reference_given_later() {
    let mut scored = row("a", true, 100, "wrong words", None);
    scored.wer = Some(0.0);
    let ranked = rank(&[scored], Some("totally different"), None);
    assert_eq!(ranked[0].wer, Some(0.0));
}

#[test]
fn the_shortlist_is_the_top_three_plus_the_best_local_engine() {
    let mut rows = Vec::new();
    for (engine, latency) in [
        ("groq:a", 100),
        ("openai:b", 200),
        ("openrouter:c", 300),
        ("deepgram:d", 400),
        ("local:slow", 900),
        ("local:slower", 1_500),
    ] {
        rows.push(row(engine, true, latency, "x", None));
    }
    let ranked = rank(&rows, None, None);
    let names: Vec<&str> = shortlist(&ranked)
        .iter()
        .map(|entry| entry.engine.as_str())
        .collect();
    assert_eq!(names, ["groq:a", "openai:b", "openrouter:c", "local:slow"]);
    // A local engine already in the top three is not listed twice.
    rows.push(row("local:fast", true, 50, "x", None));
    let ranked = rank(&rows, None, None);
    let names: Vec<&str> = shortlist(&ranked)
        .iter()
        .map(|entry| entry.engine.as_str())
        .collect();
    assert_eq!(names, ["local:fast", "groq:a", "openai:b"]);
    let text = report_text(&ranked);
    assert!(text.contains("Shortlist"));
    assert!(text.contains("local:fast"));
    assert!(report_text(&[]).contains("No comparison rows"));
}

#[test]
fn rows_append_to_a_jsonl_file_and_read_back_skipping_junk() {
    let dir = std::env::temp_dir().join(format!("wordink-compare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("logs").join(LOG_FILE);
    let first = vec![
        row("groq:turbo", true, 700, "hello", Some(0.0001)),
        row("openai:mini", false, 0, "", None),
    ];
    append(&path, &first).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "not json").unwrap();
    drop(file);
    append(&path, &first[..1]).unwrap();
    let rows = read(&path).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], first[0]);
    assert_eq!(rows[1].error.as_deref(), Some("boom"));
    let lines = std::fs::read_to_string(&path).unwrap();
    assert_eq!(lines.lines().count(), 4, "one object a line");
    // A failed row leaves its absent fields out.
    assert!(!lines.lines().nth(1).unwrap().contains("request_bytes"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_comparison_shows_metrics_texts_and_diffs() {
    let mut a = row("groq:turbo", true, 800, "the quick brown fox", Some(0.0002));
    a.request_bytes = Some(1_800_000);
    let b = row(
        "local:parakeet",
        true,
        2_400,
        "the quick brown box",
        Some(0.0),
    );
    let c = row("openai:mini", false, 0, "", None);
    let text = comparison_text(&[a, b, c], None, 100);
    assert!(text.contains("groq:turbo"));
    assert!(text.contains("1800000"));
    assert!(text.contains("failed: boom"));
    assert!(text.contains("Side by side"));
    // The first answering engine is the baseline; the others diff to it.
    assert!(text.contains("Word diff against groq:turbo"));
    assert!(text.contains("local:parakeet: the quick brown [-fox-]{+box+}"));
    // With a reference, every engine diffs to it, the first included.
    let mut a = row("groq:turbo", true, 800, "the quick brown fox", None);
    a.wer = Some(0.0);
    let text = comparison_text(&[a], Some("the quick brown fox"), 100);
    assert!(text.contains("Word diff against the reference"));
    assert!(text.contains("groq:turbo: the quick brown fox"));
}
