import Foundation

/// The persisted endpoint key shared by the app and the intents.
enum GatewayDefaults {
    static let endpointKey = "gatewayBaseURL"

    static func storedEndpoint(in defaults: UserDefaults = .standard) -> URL? {
        defaults.url(forKey: endpointKey)
    }
}

/// Gateway endpoint parsing and the KTD5 transport policy.
///
/// `https` is allowed everywhere. `http` is allowed only for the local-network
/// class — loopback/RFC-1918 IP literals, `.local` and unqualified host names,
/// and CGNAT IPs (100.64.0.0/10) — where the transport is trusted or
/// WireGuard-encrypted (Tailscale). Userinfo, query, fragment, and paths are
/// rejected: the endpoint is origin plus port.
enum GatewayConfig {
    static func normalizedEndpoint(_ input: String) throws -> URL {
        var s = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.isEmpty { throw GatewayError.invalidEndpoint("empty address") }
        if !s.contains("://") { s = "http://" + s }

        guard var comps = URLComponents(string: s) else {
            throw GatewayError.invalidEndpoint("unparseable address")
        }
        guard comps.user == nil, comps.password == nil else {
            throw GatewayError.invalidEndpoint("credentials go in the token field, not the URL")
        }
        guard comps.query == nil, comps.fragment == nil else {
            throw GatewayError.invalidEndpoint("query and fragment are not allowed")
        }
        guard let scheme = comps.scheme?.lowercased(), scheme == "http" || scheme == "https" else {
            throw GatewayError.invalidEndpoint("scheme must be http or https")
        }
        guard let host = comps.host, !host.isEmpty else {
            throw GatewayError.invalidEndpoint("missing host")
        }
        if !comps.path.isEmpty && comps.path != "/" {
            throw GatewayError.invalidEndpoint("path is not allowed — the endpoint is origin plus port")
        }
        if scheme == "http" && !isLocalNetworkHost(host) {
            throw GatewayError.invalidEndpoint("http is only allowed for LAN or Tailscale endpoints — use https, or reach the gateway via LAN/Tailscale")
        }
        comps.path = ""
        guard let url = comps.url else {
            throw GatewayError.invalidEndpoint("unparseable address")
        }
        return url
    }

    static func isLocalNetworkHost(_ host: String) -> Bool {
        let h = host.lowercased()
        if h == "localhost" || h.hasSuffix(".local") { return true }
        if h == "::1" { return true }
        if h.allSatisfy({ $0.isNumber || $0 == "." }) { return isLocalIPv4(h) }
        // IPv6 literals contain ":" not "." — without this they'd fall through to
        // the dotless-host rule and read as LAN (`::1` already returned above).
        if h.contains(":") { return false }
        return !h.contains(".") // unqualified single-label host — LAN
    }

    private static func isLocalIPv4(_ h: String) -> Bool {
        let parts = h.split(separator: ".").compactMap { Int($0) }
        guard parts.count == 4, parts.allSatisfy({ $0 <= 255 }) else { return false }
        switch parts[0] {
        case 127: return true
        case 10: return true
        case 100: return (64...127).contains(parts[1])
        case 169: return parts[1] == 254
        case 172: return (16...31).contains(parts[1])
        case 192: return parts[1] == 168
        default: return false
        }
    }
}
