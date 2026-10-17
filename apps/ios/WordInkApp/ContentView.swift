import Foundation
import Security
import SwiftUI
import WordInkIOS

/// Navigation sections shared across the iPadOS `NavigationSplitView` sidebar and the iOS `TabView`.
public enum HostDestination: String, CaseIterable, Identifiable {
    case session = "Flow Session"
    case dictionary = "Dictionary"
    case snippets = "Snippets"
    case styles = "App Styles"
    case gateway = "Gateway"

    public var id: String { rawValue }

    public var iconName: String {
        switch self {
        case .session:
            return "waveform.badge.mic"
        case .dictionary:
            return "character.book.closed"
        case .snippets:
            return "text.badge.plus"
        case .styles:
            return "slider.horizontal.3"
        case .gateway:
            return "key.radiowaves.forward"
        }
    }
}

/// Keychain storage for the `wdk_...` device token so it never lives in plaintext `UserDefaults`.
public final class KeychainTokenStore: @unchecked Sendable {
    public static let shared = KeychainTokenStore()
    private let service = "io.wordink.app.gateway"
    private let account = "device_token"

    public init() {}

    /// Stores the token, or clears it when `token` is empty. Returns `false` if the Keychain write failed.
    @discardableResult
    public func saveToken(_ token: String) -> Bool {
        let trimmed = token.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let data = trimmed.data(using: .utf8) else { return false }

        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account
        ]
        SecItemDelete(query as CFDictionary)

        guard !trimmed.isEmpty else { return true }
        var insert = query
        insert[kSecValueData as String] = data
        insert[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        return SecItemAdd(insert as CFDictionary, nil) == errSecSuccess
    }

    public func loadToken() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        guard status == errSecSuccess,
              let data = result as? Data,
              let token = String(data: data, encoding: .utf8),
              !token.isEmpty else {
            return nil
        }
        return token
    }
}

/// Shared App Group configuration helper for Voice Snippets, Per-App Flow Styles, and Gateway URL.
public final class AppGroupSettingsStore: @unchecked Sendable {
    public static let shared = AppGroupSettingsStore()
    private let defaults: UserDefaults

    public init(defaults: UserDefaults? = UserDefaults(suiteName: WordInkAppGroup.identifier)) {
        self.defaults = defaults ?? .standard
    }

    public func loadSnippets() -> [String: String] {
        if let dict = defaults.dictionary(forKey: WordInkAppGroup.snippetsKey) as? [String: String],
           !dict.isEmpty {
            return dict
        }
        return [
            "my calendly": "https://cal.com/luke",
            "my github": "https://github.com/duketopceo"
        ]
    }

    public func saveSnippets(_ snippets: [String: String]) {
        defaults.set(snippets, forKey: WordInkAppGroup.snippetsKey)
    }

    public func loadPerAppStyles() -> [String: FlowTone] {
        if let raw = defaults.dictionary(forKey: WordInkAppGroup.perAppStylesKey) as? [String: String],
           !raw.isEmpty {
            var mapped: [String: FlowTone] = [:]
            for (bundleId, toneRaw) in raw {
                if let tone = FlowTone(rawValue: toneRaw) {
                    mapped[bundleId] = tone
                }
            }
            return mapped
        }
        return [
            "com.apple.MobileSMS": .casual,
            "com.tinyspeck.chatlyio": .work,
            "com.apple.mobilemail": .email,
            "com.Termius": .code
        ]
    }

    public func savePerAppStyles(_ styles: [String: FlowTone]) {
        let raw = styles.mapValues(\.rawValue)
        defaults.set(raw, forKey: WordInkAppGroup.perAppStylesKey)
    }

    public func loadGatewayURL() -> URL {
        if let raw = defaults.string(forKey: "io.wordink.shared.gatewayURL"),
           let url = URL(string: raw) {
            return url
        }
        return URL(string: "http://127.0.0.1:8941")!
    }

    public func saveGatewayURL(_ urlString: String) {
        defaults.set(urlString.trimmingCharacters(in: .whitespacesAndNewlines), forKey: "io.wordink.shared.gatewayURL")
    }
}

/// Adaptive Universal SwiftUI interface:
/// - `NavigationSplitView` sidebar + detail columns on iPadOS (`horizontalSizeClass == .regular`)
/// - `TabView` on iPhone (`horizontalSizeClass == .compact`)
public struct ContentView: View {
    @ObservedObject public var audioManager: FlowAudioSessionManager
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    @State private var selectedDestination: HostDestination? = .session
    @State private var dictionaryTerms: [String] = []
    @State private var newDictionaryTerm: String = ""
    @State private var snippets: [String: String] = [:]
    @State private var newSnippetTrigger: String = ""
    @State private var newSnippetExpansion: String = ""
    @State private var perAppStyles: [String: FlowTone] = [:]
    @State private var newBundleId: String = ""
    @State private var newBundleTone: FlowTone = .work
    @State private var gatewayURLString: String = "http://127.0.0.1:8941"
    @State private var gatewayToken: String = ""
    @State private var tokenSavedBanner: Bool = false

    public init(audioManager: FlowAudioSessionManager) {
        self.audioManager = audioManager
    }

    public var body: some View {
        Group {
            if horizontalSizeClass == .regular {
                // iPadOS adaptive two-column layout
                NavigationSplitView {
                    List(HostDestination.allCases, selection: $selectedDestination) { destination in
                        Label(destination.rawValue, systemImage: destination.iconName)
                            .tag(destination)
                    }
                    .navigationTitle("WordInk Flow")
                } detail: {
                    destinationView(for: selectedDestination ?? .session)
                }
            } else {
                // iPhone compact TabView layout
                TabView {
                    ForEach(HostDestination.allCases) { destination in
                        NavigationStack {
                            destinationView(for: destination)
                        }
                        .tabItem {
                            Label(destination.rawValue, systemImage: destination.iconName)
                        }
                    }
                }
            }
        }
        .onAppear(perform: reloadAllSettings)
    }

    @ViewBuilder
    private func destinationView(for destination: HostDestination) -> some View {
        switch destination {
        case .session:
            sessionControlPanel
        case .dictionary:
            personalDictionaryPanel
        case .snippets:
            voiceSnippetsPanel
        case .styles:
            perAppStylesPanel
        case .gateway:
            gatewayTokenPanel
        }
    }

    // MARK: - 1. Flow Session & Whisper Mode Panel

    private var sessionControlPanel: some View {
        Form {
            Section("Warm Microphone Lease (300s)") {
                HStack {
                    Label("Session State", systemImage: "waveform.circle.fill")
                    Spacer()
                    Text(audioManager.sessionState.rawValue.uppercased())
                        .font(.caption.monospaced().weight(.bold))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(Color.accentColor.opacity(0.15), in: Capsule())
                }

                HStack {
                    Text("Remaining Lease")
                    Spacer()
                    Text("\(Int(audioManager.remainingLeaseSeconds))s")
                        .font(.body.monospacedDigit())
                        .foregroundStyle(.secondary)
                }

                Button {
                    Task {
                        await audioManager.startWarmAudioLease(
                            ttlSeconds: FlowLease.defaultTTLSeconds,
                            tone: audioManager.activeTone
                        )
                    }
                } label: {
                    Label("Start 5-Minute Warm Audio Lease", systemImage: "mic.fill")
                }

                if audioManager.sessionState != .cold {
                    Button(role: .destructive) {
                        audioManager.stopWarmAudioLease()
                    } label: {
                        Label("Stop Warm Lease", systemImage: "stop.circle")
                    }
                }
            }

            Section("Whispering Mode (+20 dB Soft-Speech Boost)") {
                Toggle(isOn: $audioManager.isWhisperModeEnabled) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Whisper Mode (+20 dB Gain)")
                        Text("Applies 10× linear software gain with soft-knee limiting for quiet offices and late-night dictation.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                Picker("Default Flow Tone", selection: $audioManager.activeTone) {
                    ForEach(FlowTone.allCases, id: \.self) { tone in
                        Text(tone.rawValue.capitalized).tag(tone)
                    }
                }
            }

            if !audioManager.lastTranscript.isEmpty {
                Section("Latest Utterance (\(audioManager.lastTranscriptionSource.rawValue))") {
                    Text(audioManager.lastTranscript)
                        .font(.body)
                }
            }
        }
        .navigationTitle("Flow Session")
    }

    // MARK: - 2. Personal Dictionary Panel

    private var personalDictionaryPanel: some View {
        Form {
            Section("Add Custom Spelling or Proper Noun") {
                HStack {
                    TextField("e.g. WordInk, Omarchy, Hyprland", text: $newDictionaryTerm)
                        .autocorrectionDisabled()
                    Button("Add") {
                        let trimmed = newDictionaryTerm.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !trimmed.isEmpty, !dictionaryTerms.contains(trimmed) else { return }
                        dictionaryTerms.append(trimmed)
                        audioManager.store.savePersonalDictionary(dictionaryTerms)
                        newDictionaryTerm = ""
                    }
                    .disabled(newDictionaryTerm.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }

            Section("Learned & Saved Terms (\(dictionaryTerms.count))") {
                if dictionaryTerms.isEmpty {
                    Text("Corrections made within 20 seconds after dictation are learned automatically.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(dictionaryTerms, id: \.self) { term in
                        Text(term)
                    }
                    .onDelete { offsets in
                        dictionaryTerms.remove(atOffsets: offsets)
                        audioManager.store.savePersonalDictionary(dictionaryTerms)
                    }
                }
            }
        }
        .navigationTitle("Personal Dictionary")
    }

    // MARK: - 3. Voice Snippets Panel

    private var voiceSnippetsPanel: some View {
        Form {
            Section("New Spoken Trigger → Expansion") {
                TextField("Spoken trigger (e.g. my calendly)", text: $newSnippetTrigger)
                    .autocorrectionDisabled()
                TextField("Expanded text (e.g. https://cal.com/luke)", text: $newSnippetExpansion)
                    .autocorrectionDisabled()
                Button("Save Snippet") {
                    let trigger = newSnippetTrigger.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
                    let expansion = newSnippetExpansion.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !trigger.isEmpty, !expansion.isEmpty else { return }
                    snippets[trigger] = expansion
                    AppGroupSettingsStore.shared.saveSnippets(snippets)
                    newSnippetTrigger = ""
                    newSnippetExpansion = ""
                }
            }

            Section("Active Voice Snippets") {
                ForEach(snippets.keys.sorted(), id: \.self) { key in
                    VStack(alignment: .leading, spacing: 4) {
                        Text("\"\(key)\"")
                            .font(.subheadline.weight(.semibold))
                        Text(snippets[key] ?? "")
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .navigationTitle("Voice Snippets")
    }

    // MARK: - 4. Per-App Flow Styles Panel

    private var perAppStylesPanel: some View {
        Form {
            Section("Add Host App Bundle Mapping") {
                TextField("Bundle ID (e.g. com.apple.MobileSMS)", text: $newBundleId)
                    .autocorrectionDisabled()
                Picker("Flow Tone", selection: $newBundleTone) {
                    ForEach(FlowTone.allCases, id: \.self) { tone in
                        Text(tone.rawValue.capitalized).tag(tone)
                    }
                }
                Button("Save Mapping") {
                    let bundle = newBundleId.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !bundle.isEmpty else { return }
                    perAppStyles[bundle] = newBundleTone
                    AppGroupSettingsStore.shared.savePerAppStyles(perAppStyles)
                    newBundleId = ""
                }
            }

            Section("Configured App Styles") {
                ForEach(perAppStyles.keys.sorted(), id: \.self) { bundleId in
                    HStack {
                        Text(bundleId)
                            .font(.subheadline.monospaced())
                        Spacer()
                        Text((perAppStyles[bundleId] ?? .work).rawValue.uppercased())
                            .font(.caption.weight(.bold))
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .navigationTitle("Per-App Flow Styles")
    }

    // MARK: - 5. Keychain Gateway Token Panel

    private var gatewayTokenPanel: some View {
        Form {
            Section("WordInk Gateway Endpoint") {
                TextField("https://gateway.example.com", text: $gatewayURLString)
                    .autocorrectionDisabled()
            }

            Section("Keychain Device Token (wdk_...)") {
                SecureField("wdk_...", text: $gatewayToken)
                    .autocorrectionDisabled()
                Button("Save Gateway Credentials to Keychain") {
                    AppGroupSettingsStore.shared.saveGatewayURL(gatewayURLString)
                    tokenSavedBanner = KeychainTokenStore.shared.saveToken(gatewayToken)
                }
                if tokenSavedBanner {
                    Label("Saved wdk_... token in iOS Keychain", systemImage: "checkmark.seal.fill")
                        .font(.caption)
                        .foregroundStyle(.green)
                }
            }

            Section("Offline Resilience") {
                Text("When offline or if the Gateway returns 5xx, WordInk automatically falls back to on-device Apple Speech / WhisperKit inside the Host App.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .navigationTitle("Gateway & Keychain")
    }

    private func reloadAllSettings() {
        dictionaryTerms = audioManager.store.loadPersonalDictionary()
        snippets = AppGroupSettingsStore.shared.loadSnippets()
        perAppStyles = AppGroupSettingsStore.shared.loadPerAppStyles()
        gatewayURLString = AppGroupSettingsStore.shared.loadGatewayURL().absoluteString
        gatewayToken = KeychainTokenStore.shared.loadToken() ?? ""
    }
}
