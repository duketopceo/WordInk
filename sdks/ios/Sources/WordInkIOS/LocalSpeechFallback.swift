import Foundation
#if canImport(Speech)
import Speech
#endif

/// Origin of a completed transcription.
public enum TranscriptionSource: String, Codable, Equatable, Sendable {
    case gateway = "gateway"
    case onDeviceAppleSpeech = "on-device-apple-speech"
    case onDeviceWhisperKit = "on-device-whisperkit"
}

/// Output returned by any `SpeechTranscriberProtocol` implementation.
public struct TranscriptionOutput: Codable, Equatable, Sendable {
    public let text: String
    public let source: TranscriptionSource

    public init(text: String, source: TranscriptionSource) {
        self.text = text
        self.source = source
    }
}

/// Unified async transcription contract shared by `GatewayTranscriptionClient`,
/// `LocalFallbackTranscriber`, and `TranscriberRouter`.
public protocol SpeechTranscriberProtocol: Sendable {
    func transcribe(
        audioData: Data,
        options: FlowRequestOptions
    ) async throws -> TranscriptionOutput
}

/// Adapter interface for on-device speech recognition engines (`WhisperKit` CoreML or Apple `SFSpeechRecognizer`).
/// Runs strictly inside the Host App (`WordInkApp`) — never inside `WordInkKeyboard` (< 48 MB memory ceiling).
public protocol OnDeviceSpeechAdapter: Sendable {
    var source: TranscriptionSource { get }
    var isAvailable: Bool { get }
    func recognize(wavData: Data, localeIdentifier: String) async throws -> String
}

/// Errors thrown by `LocalFallbackTranscriber`.
public enum LocalFallbackError: Error, Equatable, Sendable {
    case noAvailableOnDeviceEngine
    case emptyAudioBuffer
    case recognitionFailed(String)
}

/// Apple `SFSpeechRecognizer` (`requiresOnDeviceRecognition = true`) adapter for zero-download offline fallback.
public struct AppleSFSpeechAdapter: OnDeviceSpeechAdapter {
    public let source: TranscriptionSource = .onDeviceAppleSpeech
    public let localeIdentifier: String

    public init(localeIdentifier: String = "en-US") {
        self.localeIdentifier = localeIdentifier
    }

    public var isAvailable: Bool {
        #if canImport(Speech)
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: localeIdentifier)) else {
            return false
        }
        return recognizer.isAvailable && recognizer.supportsOnDeviceRecognition
        #else
        return false
        #endif
    }

    public func recognize(wavData: Data, localeIdentifier: String = "en-US") async throws -> String {
        guard !wavData.isEmpty else {
            throw LocalFallbackError.emptyAudioBuffer
        }
        #if canImport(Speech)
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: localeIdentifier)),
              recognizer.isAvailable else {
            throw LocalFallbackError.noAvailableOnDeviceEngine
        }

        let tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("wordink-fallback-\(UUID().uuidString).wav")
        try wavData.write(to: tempURL, options: .atomic)
        defer { try? FileManager.default.removeItem(at: tempURL) }

        let request = SFSpeechURLRecognitionRequest(url: tempURL)
        request.requiresOnDeviceRecognition = true
        request.shouldReportPartialResults = false

        return try await withCheckedThrowingContinuation { continuation in
            recognizer.recognitionTask(with: request) { result, error in
                if let error {
                    continuation.resume(throwing: LocalFallbackError.recognitionFailed(error.localizedDescription))
                    return
                }
                if let result, result.isFinal {
                    continuation.resume(returning: result.bestTranscription.formattedString)
                }
            }
        }
        #else
        throw LocalFallbackError.noAvailableOnDeviceEngine
        #endif
    }
}

/// Pluggable WhisperKit CoreML / Neural Engine adapter for host-app on-device transcription.
/// Accepts a closure so the host app can wire `WhisperKit.transcribe(audioPath:)` or a test stub.
public struct WhisperKitAdapter: OnDeviceSpeechAdapter {
    public let source: TranscriptionSource = .onDeviceWhisperKit
    public let modelVariant: String
    private let availabilityCheck: @Sendable () -> Bool
    private let transcribeHandler: @Sendable (Data, String) async throws -> String

    public init(
        modelVariant: String = "openai_whisper-base",
        isAvailable: @escaping @Sendable () -> Bool = { true },
        transcribeHandler: @escaping @Sendable (Data, String) async throws -> String
    ) {
        self.modelVariant = modelVariant
        self.availabilityCheck = isAvailable
        self.transcribeHandler = transcribeHandler
    }

    public var isAvailable: Bool {
        availabilityCheck()
    }

    public func recognize(wavData: Data, localeIdentifier: String = "en-US") async throws -> String {
        guard !wavData.isEmpty else {
            throw LocalFallbackError.emptyAudioBuffer
        }
        return try await transcribeHandler(wavData, localeIdentifier)
    }
}

/// On-device fallback transcriber combining an optional `WhisperKit` adapter and Apple `SFSpeechRecognizer`,
/// followed by lightweight deterministic local Flow cleanup.
public final class LocalFallbackTranscriber: SpeechTranscriberProtocol, @unchecked Sendable {
    private let adapters: [OnDeviceSpeechAdapter]
    private let localeIdentifier: String

    public init(
        adapters: [OnDeviceSpeechAdapter] = [AppleSFSpeechAdapter()],
        localeIdentifier: String = "en-US"
    ) {
        self.adapters = adapters
        self.localeIdentifier = localeIdentifier
    }

    public func transcribe(
        audioData: Data,
        options: FlowRequestOptions = FlowRequestOptions()
    ) async throws -> TranscriptionOutput {
        guard !audioData.isEmpty else {
            throw LocalFallbackError.emptyAudioBuffer
        }

        var lastError: Error = LocalFallbackError.noAvailableOnDeviceEngine
        for adapter in adapters where adapter.isAvailable {
            do {
                let rawText = try await adapter.recognize(wavData: audioData, localeIdentifier: localeIdentifier)
                let cleaned = Self.applyLightweightLocalCleanup(rawText, options: options)
                return TranscriptionOutput(text: cleaned, source: adapter.source)
            } catch {
                lastError = error
            }
        }
        throw lastError
    }

    /// Lightweight deterministic cleanup applied on-device when the gateway is unreachable.
    public static func applyLightweightLocalCleanup(_ text: String, options: FlowRequestOptions) -> String {
        guard options.tone != .verbatim, !text.isEmpty else { return text }
        var out = text

        if options.stripFillers {
            let fillerPattern = #"(?i)(?:^|\s+)\b(?:um+|uh+|erm+|ah+|hmm+)\b(?:\s*,)?"#
            out = out.replacingOccurrences(of: fillerPattern, with: " ", options: .regularExpression)
        }

        for (trigger, expansion) in options.snippets.sorted(by: { $0.key.count > $1.key.count }) {
            let escaped = NSRegularExpression.escapedPattern(for: trigger)
            let pattern = "(?i)\\b\(escaped)\\b"
            out = out.replacingOccurrences(of: pattern, with: expansion, options: .regularExpression)
        }

        out = out
            .replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespacesAndNewlines)

        switch options.tone {
        case .casual:
            var lower = out.lowercased()
            if !lower.contains(". ") && lower.hasSuffix(".") {
                lower.removeLast()
            }
            return lower
        case .work, .email:
            guard let first = out.first else { return out }
            let capitalized = String(first).uppercased() + out.dropFirst()
            if capitalized.hasSuffix(".") || capitalized.hasSuffix("!") || capitalized.hasSuffix("?") {
                return capitalized
            }
            return capitalized + "."
        case .code, .verbatim:
            return out
        }
    }
}

/// Routes transcription requests to the primary `GatewayTranscriptionClient` and automatically
/// falls back to `LocalFallbackTranscriber` when the device is offline or the gateway returns
/// a 5xx / timeout / unreachable error.
public final class TranscriberRouter: SpeechTranscriberProtocol, @unchecked Sendable {
    public let primary: SpeechTranscriberProtocol
    public let fallback: SpeechTranscriberProtocol
    private let isOffline: @Sendable () -> Bool

    public init(
        primary: SpeechTranscriberProtocol,
        fallback: SpeechTranscriberProtocol,
        isOffline: @escaping @Sendable () -> Bool = { false }
    ) {
        self.primary = primary
        self.fallback = fallback
        self.isOffline = isOffline
    }

    public func transcribe(
        audioData: Data,
        options: FlowRequestOptions = FlowRequestOptions()
    ) async throws -> TranscriptionOutput {
        if isOffline() {
            return try await fallback.transcribe(audioData: audioData, options: options)
        }

        do {
            return try await primary.transcribe(audioData: audioData, options: options)
        } catch let error as GatewayClientError where error.isFallbackEligible {
            return try await fallback.transcribe(audioData: audioData, options: options)
        } catch is URLError {
            return try await fallback.transcribe(audioData: audioData, options: options)
        }
    }
}
