import Foundation

/// Thin client over the gateway's OpenAI-compatible surface (KTD4).
struct GatewayClient {
    private let session: URLSession

    init(session: URLSession = .shared) {
        self.session = session
    }

    /// The onboarding contract both entry points share: a token is stored only
    /// after it proves it can list models against the gateway (R6).
    static func connect(baseURL: URL, token: String) async throws {
        try await GatewayClient().validate(baseURL: baseURL, token: token)
        guard KeychainStore.saveToken(token) else {
            throw GatewayError.invalidEndpoint("the keychain refused to store the device token")
        }
    }

    /// Onboarding check: a device token must be able to list models (R6).
    func validate(baseURL: URL, token: String) async throws {
        var req = URLRequest(url: baseURL.appendingPathComponent("v1/models"))
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.timeoutInterval = 10
        let (_, response) = try await perform(req)
        try checkStatus(response)
    }

    /// Multipart transcription: `file` field plus a fixed `model` value the
    /// gateway accepts and ignores (KTD4). Returns the transcript text.
    func transcribe(fileURL: URL, baseURL: URL, token: String) async throws -> String {
        let boundary = "WordInkBoundary-\(UUID().uuidString)"
        var req = URLRequest(url: baseURL.appendingPathComponent("v1/audio/transcriptions"))
        req.httpMethod = "POST"
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
        req.timeoutInterval = 60

        var body = Data()
        body.append(Data("--\(boundary)\r\n".utf8))
        body.append(Data("Content-Disposition: form-data; name=\"model\"\r\n\r\n".utf8))
        body.append(Data("whisper-1\r\n".utf8))
        body.append(Data("--\(boundary)\r\n".utf8))
        body.append(Data("Content-Disposition: form-data; name=\"file\"; filename=\"audio.m4a\"\r\n".utf8))
        body.append(Data("Content-Type: audio/m4a\r\n\r\n".utf8))
        body.append(try Data(contentsOf: fileURL))
        body.append(Data("\r\n--\(boundary)--\r\n".utf8))
        req.httpBody = body

        let (data, response) = try await perform(req)
        try checkStatus(response)
        struct Transcription: Decodable { let text: String }
        guard let decoded = try? JSONDecoder().decode(Transcription.self, from: data) else {
            throw GatewayError.decoding
        }
        return decoded.text
    }

    /// 200 is success; 401/403 map to auth; everything else to server (R5).
    private func checkStatus(_ response: HTTPURLResponse) throws {
        switch response.statusCode {
        case 200: return
        case 401, 403: throw GatewayError.auth
        default: throw GatewayError.server(response.statusCode)
        }
    }

    private func perform(_ req: URLRequest) async throws -> (Data, HTTPURLResponse) {
        do {
            let (data, response) = try await session.data(for: req)
            guard let http = response as? HTTPURLResponse else { throw GatewayError.unreachable }
            return (data, http)
        } catch {
            throw GatewayError.wrap(error)
        }
    }
}
