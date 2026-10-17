import Foundation

/// Per-utterance Wispr Flow cleanup and formatting options sent with `/v1/audio/transcriptions`.
public struct FlowRequestOptions: Codable, Equatable, Sendable {
    public var tone: FlowTone
    public var stripFillers: Bool
    public var backtrack: Bool
    public var lists: Bool
    public var whisperMode: Bool
    public var selectedText: String?
    public var snippets: [String: String]
    public var vocabulary: [String]

    public init(
        tone: FlowTone = .work,
        stripFillers: Bool = true,
        backtrack: Bool = true,
        lists: Bool = true,
        whisperMode: Bool = false,
        selectedText: String? = nil,
        snippets: [String: String] = [:],
        vocabulary: [String] = []
    ) {
        self.tone = tone
        self.stripFillers = stripFillers
        self.backtrack = backtrack
        self.lists = lists
        self.whisperMode = whisperMode
        self.selectedText = selectedText
        self.snippets = snippets
        self.vocabulary = vocabulary
    }
}

/// Software DSP gain stage for Whispering Mode (+20 dB soft-speech boost with soft-knee limiting).
public enum WhisperGainBooster {
    /// Default Whispering Mode gain in decibels (+20 dB = 10.0x linear amplitude).
    public static let defaultDecibels: Float = 20.0

    /// Convert decibels to linear amplitude multiplier: `10^(dB / 20)`.
    public static func linearGain(forDecibels decibels: Float = defaultDecibels) -> Float {
        pow(10.0, decibels / 20.0)
    }

    /// Apply a software gain boost (default +20 dB) to normalized Float32 PCM samples `[-1.0, 1.0]`
    /// with soft-knee saturation and hard clipping protection so whispered speech is amplified
    /// without harsh digital wraparound.
    public static func boostPCMFloat32(
        _ samples: [Float],
        decibels: Float = defaultDecibels,
        softKneeThreshold: Float = 0.85
    ) -> [Float] {
        guard !samples.isEmpty else { return [] }
        let gain = linearGain(forDecibels: decibels)
        return samples.map { sample in
            let amplified = sample * gain
            let sign: Float = amplified >= 0 ? 1.0 : -1.0
            let mag = abs(amplified)
            if mag <= softKneeThreshold {
                return amplified
            }
            // Soft-knee compression above threshold asymptotically approaching 1.0
            let excess = mag - softKneeThreshold
            let headroom = max(0.0001, 1.0 - softKneeThreshold)
            let compressed = softKneeThreshold + headroom * (excess / (excess + headroom))
            return sign * min(1.0, compressed)
        }
    }

    /// Apply a software gain boost (default +20 dB) to 16-bit signed PCM samples.
    public static func boostPCMInt16(
        _ samples: [Int16],
        decibels: Float = defaultDecibels
    ) -> [Int16] {
        guard !samples.isEmpty else { return [] }
        let floats = samples.map { Float($0) / Float(Int16.max) }
        let boosted = boostPCMFloat32(floats, decibels: decibels)
        return boosted.map { sample in
            let scaled = max(-1.0, min(1.0, sample)) * Float(Int16.max)
            return Int16(scaled.rounded())
        }
    }
}

/// Errors thrown by `GatewayTranscriptionClient`.
public enum GatewayClientError: Error, Equatable, Sendable {
    case invalidURL
    case missingDeviceToken
    case unauthorized(String)
    case httpError(statusCode: Int, message: String)
    case unreachable(String)
    case invalidResponse

    /// Returns `true` when the failure is caused by offline connectivity, timeout, rate limit, or
    /// a 5xx upstream/gateway error and should automatically trigger `LocalFallbackTranscriber`.
    public var isFallbackEligible: Bool {
        switch self {
        case .unreachable:
            return true
        case .httpError(let statusCode, _):
            return statusCode >= 500 || statusCode == 408 || statusCode == 429
        case .invalidURL, .missingDeviceToken, .unauthorized, .invalidResponse:
            return false
        }
    }
}

/// Transport abstraction so `GatewayTranscriptionClient` can be unit-tested without live sockets.
public protocol HTTPDataTransport: Sendable {
    func perform(_ request: URLRequest) async throws -> (Data, HTTPURLResponse)
}

/// Default `URLSession`-backed HTTP transport.
public struct URLSessionHTTPTransport: HTTPDataTransport {
    private let session: URLSession

    public init(session: URLSession = .shared) {
        self.session = session
    }

    public func perform(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
        do {
            let (data, response) = try await session.data(for: request)
            guard let http = response as? HTTPURLResponse else {
                throw GatewayClientError.invalidResponse
            }
            return (data, http)
        } catch let error as GatewayClientError {
            throw error
        } catch {
            throw GatewayClientError.unreachable(error.localizedDescription)
        }
    }
}

/// Client for `@wordink/server`'s OpenAI-compatible `POST /v1/audio/transcriptions` endpoint.
public final class GatewayTranscriptionClient: SpeechTranscriberProtocol, @unchecked Sendable {
    public let baseURL: URL
    public let deviceToken: String
    public let model: String
    private let transport: HTTPDataTransport

    public init(
        baseURL: URL,
        deviceToken: String,
        model: String = "whisper-1",
        transport: HTTPDataTransport = URLSessionHTTPTransport()
    ) {
        self.baseURL = baseURL
        self.deviceToken = deviceToken.trimmingCharacters(in: .whitespacesAndNewlines)
        self.model = model
        self.transport = transport
    }

    /// Construct a `multipart/form-data` `POST /v1/audio/transcriptions` request with
    /// `Bearer wdk_...` authorization and Wispr Flow cleanup fields.
    public func buildMultipartRequest(
        audioData: Data,
        filename: String = "dictation.wav",
        mimeType: String = "audio/wav",
        options: FlowRequestOptions = FlowRequestOptions(),
        boundary: String = "WordInkBoundary-\(UUID().uuidString)"
    ) throws -> URLRequest {
        guard !deviceToken.isEmpty else {
            throw GatewayClientError.missingDeviceToken
        }

        let endpointURL: URL
        if baseURL.path.hasSuffix("/v1/audio/transcriptions") {
            endpointURL = baseURL
        } else if baseURL.path.hasSuffix("/v1") {
            endpointURL = baseURL.appendingPathComponent("audio/transcriptions")
        } else {
            endpointURL = baseURL.appendingPathComponent("v1/audio/transcriptions")
        }

        var request = URLRequest(url: endpointURL)
        request.httpMethod = "POST"
        request.setValue("Bearer \(deviceToken)", forHTTPHeaderField: "Authorization")
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
        request.setValue(options.tone.rawValue, forHTTPHeaderField: "X-WordInk-Tone")
        request.setValue(String(options.stripFillers), forHTTPHeaderField: "X-WordInk-Strip-Fillers")
        request.setValue(String(options.backtrack), forHTTPHeaderField: "X-WordInk-Backtrack")
        request.setValue(String(options.lists), forHTTPHeaderField: "X-WordInk-Lists")

        var body = Data()

        func appendField(name: String, value: String) {
            body.append("--\(boundary)\r\n".data(using: .utf8)!)
            body.append("Content-Disposition: form-data; name=\"\(name)\"\r\n\r\n".data(using: .utf8)!)
            body.append("\(value)\r\n".data(using: .utf8)!)
        }

        // Standard OpenAI fields
        appendField(name: "model", value: model)
        appendField(name: "response_format", value: "json")

        if !options.vocabulary.isEmpty {
            appendField(name: "prompt", value: options.vocabulary.joined(separator: ", "))
        }

        // WordInk Flow extension fields
        appendField(name: "tone", value: options.tone.rawValue)
        appendField(name: "stripFillers", value: String(options.stripFillers))
        appendField(name: "backtrack", value: String(options.backtrack))
        appendField(name: "lists", value: String(options.lists))

        if options.whisperMode {
            appendField(name: "whisperMode", value: "true")
        }

        if let selectedText = options.selectedText, !selectedText.isEmpty {
            appendField(name: "selectedText", value: selectedText)
        }

        if !options.snippets.isEmpty,
           let snippetsJSON = try? JSONSerialization.data(withJSONObject: options.snippets, options: [.sortedKeys]),
           let snippetsString = String(data: snippetsJSON, encoding: .utf8) {
            appendField(name: "snippets", value: snippetsString)
        }

        // Audio file part
        body.append("--\(boundary)\r\n".data(using: .utf8)!)
        body.append(
            "Content-Disposition: form-data; name=\"file\"; filename=\"\(filename)\"\r\n".data(using: .utf8)!
        )
        body.append("Content-Type: \(mimeType)\r\n\r\n".data(using: .utf8)!)
        body.append(audioData)
        body.append("\r\n".data(using: .utf8)!)
        body.append("--\(boundary)--\r\n".data(using: .utf8)!)

        request.httpBody = body
        return request
    }

    public func transcribe(
        audioData: Data,
        options: FlowRequestOptions = FlowRequestOptions()
    ) async throws -> TranscriptionOutput {
        let request = try buildMultipartRequest(audioData: audioData, options: options)
        let (data, response) = try await transport.perform(request)

        switch response.statusCode {
        case 200:
            struct TranscriptionResponse: Decodable {
                let text: String
            }
            guard let decoded = try? JSONDecoder().decode(TranscriptionResponse.self, from: data) else {
                throw GatewayClientError.invalidResponse
            }
            return TranscriptionOutput(text: decoded.text, source: .gateway)
        case 401, 403:
            let message = Self.extractErrorMessage(from: data) ?? "Unauthorized device token"
            throw GatewayClientError.unauthorized(message)
        default:
            let message = Self.extractErrorMessage(from: data) ?? "HTTP \(response.statusCode)"
            throw GatewayClientError.httpError(statusCode: response.statusCode, message: message)
        }
    }

    private static func extractErrorMessage(from data: Data) -> String? {
        struct ErrorEnvelope: Decodable {
            struct Inner: Decodable {
                let message: String?
            }
            let error: Inner?
        }
        if let env = try? JSONDecoder().decode(ErrorEnvelope.self, from: data),
           let msg = env.error?.message {
            return msg
        }
        return String(data: data, encoding: .utf8)
    }
}
