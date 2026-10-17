import SwiftUI
import WordInkIOS

#if canImport(UIKit)
import UIKit
#endif

/// Universal iOS + iPadOS companion host application for the WordInk Voice Keyboard.
/// Owns the background `AVAudioSession` warm lease (300 s), Live Activity (`ActivityKit`),
/// Whispering-mode +20 dB software gain boost, and `POST /v1/audio/transcriptions` gateway upload.
@main
public struct WordInkApp: App {
    @StateObject private var audioManager = FlowAudioSessionManager.shared
    @Environment(\.scenePhase) private var scenePhase

    public init() {}

    public var body: some Scene {
        WindowGroup {
            ContentView(audioManager: audioManager)
                .onOpenURL { url in
                    handleIncomingDeepLink(url)
                }
        }
        .onChange(of: scenePhase) { _, newPhase in
            audioManager.handleScenePhaseChange(newPhase)
        }
    }

    /// Handles `wordink://flow/activate` (as well as `/warm` and `/start`) deep links launched from
    /// `WordInkKeyboard` when the background microphone lease is cold or expired.
    ///
    /// Example URL:
    /// `wordink://flow/activate?tone=work&bundleId=com.tinyspeck.chatlyio&whisper=1&returnScheme=slack`
    private func handleIncomingDeepLink(_ url: URL) {
        guard url.scheme?.lowercased() == "wordink",
              url.host?.lowercased() == "flow" else {
            return
        }

        let path = url.path.lowercased()
        guard path == "/activate" || path == "/warm" || path == "/start" || path.isEmpty else {
            return
        }

        let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        let queryItems = components?.queryItems ?? []

        func queryValue(_ name: String) -> String? {
            queryItems.first(where: { $0.name.caseInsensitiveCompare(name) == .orderedSame })?.value
        }

        let requestedTone = queryValue("tone").flatMap(FlowTone.init(rawValue:))
        let hostBundleId = queryValue("bundleId")
        let whisperFlag = queryValue("whisper")
        let returnScheme = queryValue("returnScheme") ?? queryValue("return")
        let ttlSeconds = queryValue("lease").flatMap(TimeInterval.init) ?? FlowLease.defaultTTLSeconds

        if let whisperFlag {
            audioManager.isWhisperModeEnabled = (whisperFlag == "1" || whisperFlag.lowercased() == "true")
        }

        let resolvedTone = requestedTone ?? ContextToneMapper.tone(forBundleId: hostBundleId)

        Task { @MainActor in
            await audioManager.startWarmAudioLease(
                ttlSeconds: ttlSeconds,
                tone: resolvedTone,
                hostBundleId: hostBundleId
            )

            await FlowLiveActivityCoordinator.shared.startOrUpdate(
                state: audioManager.sessionState,
                tone: resolvedTone,
                whisperMode: audioManager.isWhisperModeEnabled,
                leaseExpiresAt: Date().addingTimeInterval(ttlSeconds)
            )

            // Optionally jump back to the calling app when a valid return URL scheme is supplied.
            if let returnScheme = Self.validatedReturnScheme(returnScheme),
               let returnURL = URL(string: "\(returnScheme)://") {
                #if canImport(UIKit)
                await UIApplication.shared.open(returnURL)
                #endif
            }
        }
    }

    /// Validates that a `returnScheme` contains only safe RFC 3986 scheme characters and never loops back to `wordink`.
    public static func validatedReturnScheme(_ raw: String?) -> String? {
        guard let raw = raw?.trimmingCharacters(in: .whitespacesAndNewlines),
              !raw.isEmpty else {
            return nil
        }
        let cleaned = raw.replacingOccurrences(of: "://", with: "")
        guard cleaned.lowercased() != "wordink",
              cleaned.range(of: #"^[A-Za-z][A-Za-z0-9+.-]{0,31}$"#, options: .regularExpression) != nil else {
            return nil
        }
        return cleaned
    }
}
