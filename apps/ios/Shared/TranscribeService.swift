import Foundation

/// file -> transcript through the stored gateway configuration.
/// Shared by the in-app upload path and the headless intents (KTD6).
struct TranscribeService {
    var client: GatewayClient
    var endpoint: () -> URL?
    var token: () -> String?

    init(client: GatewayClient = .init(),
         endpoint: @escaping () -> URL? = { GatewayDefaults.storedEndpoint() },
         token: @escaping () -> String? = { KeychainStore.loadToken() }) {
        self.client = client
        self.endpoint = endpoint
        self.token = token
    }

    func transcribe(fileURL: URL) async throws -> String {
        guard let base = endpoint() else {
            throw GatewayError.invalidEndpoint("no gateway configured")
        }
        guard let t = token(), !t.isEmpty else {
            throw GatewayError.auth
        }
        return try await client.transcribe(fileURL: fileURL, baseURL: base, token: t)
    }
}
