import AppIntents
import Foundation

/// Opens the app straight into recording (KTD6 foreground path). Arming a
/// defaults flag is how the intent tells the UI to begin capture on appear —
/// same process, no App Group needed in v1.
struct StartDictationIntent: AppIntent {
    static var title: LocalizedStringResource = "Dictate"
    static var openAppWhenRun: Bool = true

    func perform() async throws -> some IntentResult {
        PendingDictation.arm()
        return .result()
    }
}

enum PendingDictation {
    static let key = "pendingDictationOnLaunch"

    static func arm(in defaults: UserDefaults = .standard) {
        defaults.set(true, forKey: key)
    }

    static func consume(in defaults: UserDefaults = .standard) -> Bool {
        let pending = defaults.bool(forKey: key)
        if pending { defaults.set(false, forKey: key) }
        return pending
    }
}
