import AppIntents

/// Siri / Shortcuts / Action Button surfaces (AE4).
struct WordInkShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: StartDictationIntent(),
            phrases: [
                "Dictate with \(.applicationName)",
                "Start dictation in \(.applicationName)",
            ],
            shortTitle: "Dictate",
            systemImageName: "mic.fill"
        )
        AppShortcut(
            intent: TranscribeAudioIntent(),
            phrases: [
                "Transcribe audio with \(.applicationName)",
            ],
            shortTitle: "Transcribe audio",
            systemImageName: "waveform"
        )
    }
    static var shortcutTileColor: ShortcutTileColor = .blue
}
