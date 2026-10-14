import AppIntents

struct StartDictationIntent: AppIntent {
    static var title: LocalizedStringResource = "Dictate"
    static var openAppWhenRun: Bool = true

    func perform() async throws -> some IntentResult {
        .result()
    }
}
