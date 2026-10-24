import SwiftUI

@main
struct WordInkRemoteApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}

struct ContentView: View {
    @AppStorage(GatewayDefaults.endpointKey) private var baseURL: URL?
    @State private var pairing = false
    @State private var pairSucceeded = false
    @State private var pairError: GatewayError?

    var body: some View {
        NavigationStack {
            if baseURL != nil {
                DictationView()
            } else {
                OnboardingView()
            }
        }
        // `wordink-remote://pair` deep links run the same validate-then-persist
        // contract as manual onboarding — no field ever holds the token.
        .onOpenURL { url in pair(from: url) }
        .disabled(pairing)
        .overlay {
            if pairing { ProgressView("Pairing…") }
        }
        .alert("Paired", isPresented: $pairSucceeded) {
            Button("OK") {}
        } message: {
            // Name the gateway — a hostile QR must not repoint the phone invisibly.
            Text("This iPhone is connected to \(baseURL?.host ?? "your gateway").")
        }
        // isPresented, not item: — the Xcode SDK resolves the item: alert
        // overload only for Identifiable items and xtool's toolchain accepts
        // it unconstrained, so the portable form is the Bool binding.
        .alert("Pairing failed", isPresented: Binding(
            get: { pairError != nil },
            set: { if !$0 { pairError = nil } },
        )) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(pairError?.message ?? "")
        }
    }

    private func pair(from url: URL) {
        // .disabled does not gate .onOpenURL — a second link mid-flight would
        // double the validation request and keychain write.
        guard !pairing else { return }
        pairing = true
        Task {
            do {
                let (endpoint, token) = try PairingLink.parse(url)
                try await GatewayClient.connect(baseURL: endpoint, token: token)
                baseURL = endpoint
                pairSucceeded = true
            } catch {
                pairError = GatewayError.wrap(error)
            }
            pairing = false
        }
    }
}
