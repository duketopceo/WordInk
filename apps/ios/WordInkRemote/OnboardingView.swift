import SwiftUI

/// Gateway URL + device token entry, validated against /v1/models before
/// the app becomes usable (F1, R6). Revisiting this view rotates the token.
struct OnboardingView: View {
    @AppStorage(GatewayDefaults.endpointKey) private var baseURL: URL?
    @State private var urlText = ""
    @State private var tokenText = ""
    @State private var validating = false
    @State private var error: GatewayError?

    var body: some View {
        Form {
            Section("Gateway") {
                TextField("Gateway URL (e.g. https://host.ts.net)", text: $urlText)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
                SecureField("Device token", text: $tokenText)
            }
            if let error {
                Section {
                    Text(error.message)
                        .foregroundStyle(.red)
                }
            }
            Section {
                Button(validating ? "Checking..." : "Connect") { connect() }
                    .disabled(urlText.isEmpty || tokenText.isEmpty || validating)
            }
            Section {
                Text("The gateway must be reachable over HTTPS, your LAN, or your tailnet — e.g. `tailscale serve --bg 8941` on the gateway host. One tap instead: run `wordink-gateway pair` there and scan the QR it prints.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
        .navigationTitle("Connect to your gateway")
        .onAppear {
            if let baseURL { urlText = baseURL.absoluteString }
        }
    }

    private func connect() {
        validating = true
        error = nil
        Task {
            do {
                let url = try GatewayConfig.normalizedEndpoint(urlText)
                try await GatewayClient.connect(baseURL: url, token: tokenText)
                baseURL = url
            } catch let thrown {
                error = GatewayError.wrap(thrown)
            }
            validating = false
        }
    }
}
