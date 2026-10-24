import Foundation

/// Named failure causes surfaced verbatim to the user (R5, R9).
enum GatewayError: Error, Equatable, LocalizedError, Identifiable {
    /// The endpoint fails the transport policy or cannot be parsed; carries the reason.
    case invalidEndpoint(String)
    /// DNS, connect, TLS, or timeout failure — reads as reachability per R9.
    case unreachable
    /// The gateway rejected the device token.
    case auth
    /// The gateway answered with an error status; carries the HTTP code.
    case server(Int)
    /// The response body was not the expected JSON shape.
    case decoding
    /// Microphone capture failed before upload; carries the reason.
    case recording(String)

    /// LocalizedError so Shortcuts shows the real cause when an intent throws.
    var errorDescription: String? { message }

    /// Identifiable so SwiftUI's `alert(item:)` / `sheet(item:)` bindings work —
    /// the Xcode SDK only resolves that overload for Identifiable items.
    var id: String { message }

    /// Maps an arbitrary thrown error into the taxonomy: a GatewayError passes
    /// through unchanged; anything else reads as reachability (R9).
    static func wrap(_ error: Error) -> GatewayError {
        (error as? GatewayError) ?? .unreachable
    }

    var message: String {
        switch self {
        case .invalidEndpoint(let why):
            return "Invalid gateway address: \(why)"
        case .unreachable:
            return "Can't reach the gateway. It must be on your LAN, your tailnet, or HTTPS — check the address, or connect via VPN/Tailscale."
        case .auth:
            return "The gateway rejected the device token. Rotate or re-enter it."
        case .server(let code):
            return "The gateway returned an error (HTTP \(code))."
        case .decoding:
            return "The gateway's response wasn't understood."
        case .recording(let why):
            return "Recording failed: \(why)"
        }
    }
}
