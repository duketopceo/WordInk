import Foundation

/// App Group identifier shared between the WordInk Host App and the Keyboard Extension.
public enum WordInkAppGroup {
    public static let identifier = "group.io.wordink.shared"
    public static let payloadKey = "io.wordink.shared.flow.payload"
    public static let personalDictionaryKey = "io.wordink.shared.flow.dictionary"
}

/// Cross-process Darwin notification names used to signal between the Keyboard Extension and Host App.
public enum FlowDarwinNotification {
    public static let startRecording = "io.wordink.darwin.startRecording"
    public static let stopRecording = "io.wordink.darwin.stopRecording"
    public static let transcriptReady = "io.wordink.darwin.transcriptReady"
    public static let leaseUpdated = "io.wordink.darwin.leaseUpdated"
}

/// Lifecycle states of the shared two-process Wispr Flow session.
public enum FlowSessionState: String, Codable, Equatable, Sendable {
    case cold
    case warming
    case listening
    case transcribing
    case readyToInsert
    case failed
}

/// Formatting tone presets matching `@wordink/server` Flow cleanup.
public enum FlowTone: String, Codable, Equatable, CaseIterable, Sendable {
    case casual
    case work
    case email
    case code
    case verbatim
}

/// Warm-microphone background audio lease held by the Host App.
public struct FlowLease: Codable, Equatable, Sendable {
    /// Default warm-microphone lease TTL (300 seconds / 5 minutes).
    public static let defaultTTLSeconds: TimeInterval = 300

    /// Epoch timestamp (seconds since 1970) when the host app last refreshed its background audio lease.
    public var acquiredAt: TimeInterval
    /// Lease time-to-live in seconds (defaults to 300 seconds).
    public var ttlSeconds: TimeInterval

    public init(acquiredAt: TimeInterval, ttlSeconds: TimeInterval = FlowLease.defaultTTLSeconds) {
        self.acquiredAt = acquiredAt
        self.ttlSeconds = ttlSeconds
    }

    /// Epoch timestamp when this lease expires.
    public var expiresAt: TimeInterval {
        acquiredAt + ttlSeconds
    }

    /// Returns `true` when the background microphone session is still warm at `now`.
    public func isWarm(at now: TimeInterval) -> Bool {
        ttlSeconds > 0 && now >= acquiredAt && now < expiresAt
    }

    /// Remaining seconds before lease expiration at `now` (`0` if expired).
    public func remainingSeconds(at now: TimeInterval) -> TimeInterval {
        guard isWarm(at: now) else { return 0 }
        return expiresAt - now
    }
}

/// JSON-codable payload stored in the `group.io.wordink.shared` container (`UserDefaults` or shared file).
/// Uses a monotonically increasing ring-buffer sequence number `seq` so the keyboard extension
/// never inserts the same utterance twice.
public struct SharedFlowPayload: Codable, Equatable, Sendable {
    public var state: FlowSessionState
    public var lease: FlowLease?
    /// Monotonically increasing utterance sequence number. Incremented each time `readyToInsert` is published.
    public var seq: UInt64
    /// Cleaned transcript text awaiting insertion by `textDocumentProxy.insertText`.
    public var transcript: String?
    /// Active formatting tone requested by the keyboard extension.
    public var activeTone: FlowTone
    /// Host bundle identifier captured by the keyboard extension, if available.
    public var hostBundleId: String?
    /// Optional error message when `state == .failed`.
    public var errorMessage: String?
    /// Epoch timestamp of the last state transition.
    public var updatedAt: TimeInterval

    public init(
        state: FlowSessionState = .cold,
        lease: FlowLease? = nil,
        seq: UInt64 = 0,
        transcript: String? = nil,
        activeTone: FlowTone = .work,
        hostBundleId: String? = nil,
        errorMessage: String? = nil,
        updatedAt: TimeInterval = 0
    ) {
        self.state = state
        self.lease = lease
        self.seq = seq
        self.transcript = transcript
        self.activeTone = activeTone
        self.hostBundleId = hostBundleId
        self.errorMessage = errorMessage
        self.updatedAt = updatedAt
    }
}

/// Abstraction over the shared App Group key-value store so the controller and learner can be
/// unit-tested deterministically without requiring an entitled iOS simulator container.
public protocol AppGroupStore: AnyObject {
    func loadPayload() -> SharedFlowPayload
    func savePayload(_ payload: SharedFlowPayload)
    func loadPersonalDictionary() -> [String]
    func savePersonalDictionary(_ terms: [String])
}

/// In-memory store implementation for unit tests and previews.
public final class InMemoryAppGroupStore: AppGroupStore {
    private var payloadData: Data?
    private var dictionary: [String] = []
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    public init(initialPayload: SharedFlowPayload = SharedFlowPayload(), initialDictionary: [String] = []) {
        self.payloadData = try? encoder.encode(initialPayload)
        self.dictionary = initialDictionary
    }

    public func loadPayload() -> SharedFlowPayload {
        guard let data = payloadData,
              let decoded = try? decoder.decode(SharedFlowPayload.self, from: data) else {
            return SharedFlowPayload()
        }
        return decoded
    }

    public func savePayload(_ payload: SharedFlowPayload) {
        payloadData = try? encoder.encode(payload)
    }

    public func loadPersonalDictionary() -> [String] {
        dictionary
    }

    public func savePersonalDictionary(_ terms: [String]) {
        dictionary = terms
    }
}

/// `UserDefaults(suiteName: "group.io.wordink.shared")`-backed store for production iOS targets.
public final class UserDefaultsAppGroupStore: AppGroupStore {
    private let defaults: UserDefaults
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    public init?(suiteName: String = WordInkAppGroup.identifier) {
        guard let suite = UserDefaults(suiteName: suiteName) else {
            return nil
        }
        self.defaults = suite
    }

    public init(defaults: UserDefaults) {
        self.defaults = defaults
    }

    public func loadPayload() -> SharedFlowPayload {
        guard let data = defaults.data(forKey: WordInkAppGroup.payloadKey),
              let decoded = try? decoder.decode(SharedFlowPayload.self, from: data) else {
            return SharedFlowPayload()
        }
        return decoded
    }

    public func savePayload(_ payload: SharedFlowPayload) {
        if let data = try? encoder.encode(payload) {
            defaults.set(data, forKey: WordInkAppGroup.payloadKey)
        }
    }

    public func loadPersonalDictionary() -> [String] {
        defaults.stringArray(forKey: WordInkAppGroup.personalDictionaryKey) ?? []
    }

    public func savePersonalDictionary(_ terms: [String]) {
        defaults.set(terms, forKey: WordInkAppGroup.personalDictionaryKey)
    }
}
