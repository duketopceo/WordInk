import SwiftUI

/// Hold-to-talk capture surface: dictate -> gateway -> clipboard (F1/F2).
/// Tap-toggle: a short tap keeps recording with a Stop control as the
/// accessible alternative to press-and-hold.
struct DictationView: View {
    @StateObject private var vm = DictationViewModel()
    @ObservedObject private var history = HistoryStore.shared
    @State private var pressedAt: Date?
    @State private var showSettings = false
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
        VStack(spacing: 32) {
            statusLabel

            recordButton

            if vm.state == .recording {
                Button("Stop") { vm.stopAndTranscribe() }
                    .buttonStyle(.bordered)
            }

            if case .done = vm.state {
                Button("Dictate again") { vm.reset() }
                    .buttonStyle(.bordered)
            }

            NavigationLink("History") {
                HistoryView(history: history)
            }
            .foregroundStyle(.secondary)
        }
        .padding()
        .navigationTitle("WordInk Remote")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { showSettings = true } label: {
                    Image(systemName: "gear")
                }
            }
        }
        .sheet(isPresented: $showSettings) { SettingsView() }
        .onAppear(perform: consumePendingDictation)
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { consumePendingDictation() }
        }
    }

    /// StartDictationIntent (control widget / Siri / Action Button) arms a
    /// flag, then opens the app — consume it once and begin capture.
    private func consumePendingDictation() {
        if PendingDictation.consume() { vm.startRecording() }
    }

    private var statusLabel: some View {
        Group {
            switch vm.state {
            case .ready:
                Text("Hold to dictate, or tap to start")
                    .foregroundStyle(.secondary)
            case .recording:
                Text("Listening...")
                    .foregroundStyle(.red)
            case .uploading:
                Text("Transcribing...")
            case .done(let text):
                VStack(spacing: 8) {
                    Text("Copied to clipboard")
                        .foregroundStyle(.green)
                    Text(text)
                        .lineLimit(3)
                        .foregroundStyle(.secondary)
                }
            case .failed(let error):
                Text(error.message)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
            }
        }
    }

    private var recordButton: some View {
        Circle()
            .fill(vm.state == .recording ? Color.red : Color.accentColor)
            .frame(width: 88, height: 88)
            .overlay {
                Image(systemName: vm.state == .recording ? "waveform" : "mic.fill")
                    .font(.system(size: 32))
                    .foregroundStyle(.white)
            }
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { _ in
                        if pressedAt == nil {
                            pressedAt = .now
                            vm.startRecording()
                        }
                    }
                    .onEnded { _ in
                        let held = pressedAt.map { Date().timeIntervalSince($0) } ?? 0
                        pressedAt = nil
                        // Release after a real hold stops; a quick tap keeps
                        // recording (toggle mode, Stop button ends it).
                        if held > 0.4 {
                            vm.stopAndTranscribe()
                        }
                    }
            )
    }
}

/// Token rotation / disconnect surface reachable from the gear button.
struct SettingsView: View {
    @AppStorage(GatewayDefaults.endpointKey) private var baseURL: URL?
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack {
                OnboardingView()
                Button("Disconnect", role: .destructive) {
                    KeychainStore.deleteToken()
                    baseURL = nil
                    dismiss()
                }
                .padding(.bottom)
            }
        }
    }
}
