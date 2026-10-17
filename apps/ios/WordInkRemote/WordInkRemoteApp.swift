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

    var body: some View {
        NavigationStack {
            if baseURL != nil {
                DictationView()
            } else {
                OnboardingView()
            }
        }
    }
}
