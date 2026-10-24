import Foundation

/// Parses `wordink-remote://pair?url=<endpoint>&token=<device-token>` deep links
/// minted by `wordink-gateway pair` — the camera-scan onboarding path (R6).
/// `wordink://` itself belongs to WordInkApp's flow links, hence the distinct scheme.
enum PairingLink {
    static let scheme = "wordink-remote"

    /// The endpoint and device token carried by the link. The endpoint is
    /// normalized under the same transport policy onboarding applies.
    static func parse(_ url: URL) throws -> (endpoint: URL, token: String) {
        guard url.scheme?.lowercased() == scheme else {
            throw GatewayError.invalidEndpoint("not a \(scheme) link")
        }
        guard url.host == "pair" else {
            throw GatewayError.invalidEndpoint("unknown \(scheme) action")
        }
        guard let comps = URLComponents(url: url, resolvingAgainstBaseURL: false),
              let rawEndpoint = comps.queryItems?.first(where: { $0.name == "url" })?.value,
              let token = comps.queryItems?.first(where: { $0.name == "token" })?.value,
              !token.isEmpty else {
            throw GatewayError.invalidEndpoint("pair link is missing its endpoint or token")
        }
        let endpoint = try GatewayConfig.normalizedEndpoint(rawEndpoint)
        return (endpoint, token)
    }
}
