import Foundation

/// Actions emitted by `KeyboardFlowController` in response to user mic interactions.
public enum KeyboardMicAction: Equatable, Sendable {
    /// The background audio lease is cold or expired; the keyboard extension must open the Host App
    /// via this deep-link URL to warm the background audio session and start the Live Activity.
    case openHostApp(URL)
    /// The background audio lease is warm; post `FlowDarwinNotification.startRecording` over IPC.
    case startRecordingIPC(notificationName: String)
    /// The user released the mic button; post `FlowDarwinNotification.stopRecording` over IPC.
    case stopRecordingIPC(notificationName: String)
    /// No-op transition (e.g., releasing the mic when not recording).
    case none
}

/// Result of polling the shared App Group payload for a newly completed utterance.
public struct PollInsertionResult: Equatable, Sendable {
    /// Text to pass to `textDocumentProxy.insertText(_:)`, or `nil` if no unseen utterance is ready.
    public let textToInsert: String?
    /// Updated sequence watermark after consuming any new utterance.
    public let lastSeq: UInt64

    public init(textToInsert: String?, lastSeq: UInt64) {
        self.textToInsert = textToInsert
        self.lastSeq = lastSeq
    }
}

/// Pure, deterministic state machine for the WordInk iOS Keyboard Extension.
/// Stays well within the 48 MB Keyboard Extension memory ceiling by delegating audio capture
/// and network upload to the Host App over `AppGroupStore`.
public final class KeyboardFlowController {
    private let store: AppGroupStore
    private let deepLinkScheme: String

    public init(store: AppGroupStore, deepLinkScheme: String = "wordink") {
        self.store = store
        self.deepLinkScheme = deepLinkScheme
    }

    /// Handle a mic button press in the keyboard extension.
    /// - Returns `.openHostApp(URL)` when the lease is missing, cold, or expired at `now`.
    /// - Returns `.startRecordingIPC` when the lease is warm, transitioning shared state to `.listening`.
    public func onMicTap(
        now: TimeInterval,
        tone: FlowTone = .work,
        hostBundleId: String? = nil
    ) -> KeyboardMicAction {
        var payload = store.loadPayload()
        payload.activeTone = tone
        if let hostBundleId {
            payload.hostBundleId = hostBundleId
        }

        guard let lease = payload.lease, lease.isWarm(at: now), payload.state != .cold else {
            payload.state = .warming
            payload.updatedAt = now
            store.savePayload(payload)
            return .openHostApp(buildWarmDeepLink(tone: tone, hostBundleId: hostBundleId))
        }

        payload.state = .listening
        payload.errorMessage = nil
        payload.updatedAt = now
        store.savePayload(payload)
        return .startRecordingIPC(notificationName: FlowDarwinNotification.startRecording)
    }

    /// Handle a mic button release (or stop tap) in the keyboard extension.
    /// Transitions a `.listening` session to `.transcribing` and signals the Host App to finalize audio.
    public func onMicRelease(now: TimeInterval) -> KeyboardMicAction {
        var payload = store.loadPayload()
        guard payload.state == .listening else {
            return .none
        }
        payload.state = .transcribing
        payload.updatedAt = now
        store.savePayload(payload)
        return .stopRecordingIPC(notificationName: FlowDarwinNotification.stopRecording)
    }

    /// Poll the shared App Group payload for a newly transcribed utterance.
    /// Never returns the same utterance twice: only a payload with `seq > lastSeq` and a non-empty
    /// transcript is consumed, advancing `lastSeq` to `payload.seq`.
    public func pollInsertion(lastSeq: UInt64) -> PollInsertionResult {
        var payload = store.loadPayload()
        guard payload.state == .readyToInsert,
              payload.seq > lastSeq,
              let text = payload.transcript,
              !text.isEmpty else {
            return PollInsertionResult(textToInsert: nil, lastSeq: lastSeq)
        }

        let newSeq = payload.seq
        payload.transcript = nil
        store.savePayload(payload)
        return PollInsertionResult(textToInsert: text, lastSeq: newSeq)
    }

    private func buildWarmDeepLink(tone: FlowTone, hostBundleId: String?) -> URL {
        var components = URLComponents()
        components.scheme = deepLinkScheme
        components.host = "flow"
        components.path = "/warm"
        var queryItems = [URLQueryItem(name: "tone", value: tone.rawValue)]
        if let hostBundleId, !hostBundleId.isEmpty {
            queryItems.append(URLQueryItem(name: "bundleId", value: hostBundleId))
        }
        components.queryItems = queryItems
        return components.url ?? URL(string: "\(deepLinkScheme)://flow/warm")!
    }
}
