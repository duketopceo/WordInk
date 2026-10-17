import Foundation
import SwiftUI
import WordInkIOS

#if canImport(AVFAudio)
import AVFAudio
#endif
#if canImport(AppIntents)
import AppIntents
#endif

/// Manages the Host App's background `AVAudioSession`, 300-second warm microphone lease,
/// Whispering Mode (+20 dB software gain boost), Darwin IPC listener, and Gateway/On-Device upload.
@MainActor
public final class FlowAudioSessionManager: ObservableObject {
    public static let shared = FlowAudioSessionManager()

    @Published public private(set) var sessionState: FlowSessionState = .cold
    @Published public private(set) var activeLease: FlowLease?
    @Published public private(set) var remainingLeaseSeconds: TimeInterval = 0
    @Published public var activeTone: FlowTone = .work
    @Published public var isWhisperModeEnabled: Bool = false {
        didSet {
            persistSharedPayload()
        }
    }
    @Published public var lastTranscript: String = ""
    @Published public var lastTranscriptionSource: TranscriptionSource = .gateway
    @Published public var lastErrorMessage: String?

    public let store: AppGroupStore
    private var leaseTimer: Timer?
    private var recordedSamples: [Float] = []
    private let sampleRate: Int = 16_000

    #if canImport(AVFAudio)
    private let audioEngine = AVAudioEngine()
    private var isTapInstalled = false
    #endif

    public init(
        store: AppGroupStore = UserDefaultsAppGroupStore() ?? InMemoryAppGroupStore()
    ) {
        self.store = store
        let initial = store.loadPayload()
        self.sessionState = initial.state
        self.activeLease = initial.lease
        self.activeTone = initial.activeTone
        self.isWhisperModeEnabled = initial.whisperMode
        registerDarwinNotificationObservers()
    }

    /// Starts or refreshes the 300-second background `AVAudioSession` warm lease.
    public func startWarmAudioLease(
        ttlSeconds: TimeInterval = FlowLease.defaultTTLSeconds,
        tone: FlowTone? = nil,
        hostBundleId: String? = nil
    ) async {
        if let tone {
            self.activeTone = tone
        }

        #if canImport(AVFAudio)
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(
                .playAndRecord,
                mode: .measurement,
                options: [.mixWithOthers, .allowBluetooth, .defaultToSpeaker]
            )
            try session.setActive(true, options: .notifyOthersOnDeactivation)
            installAudioEngineTapIfNeeded()
        } catch {
            self.sessionState = .failed
            self.lastErrorMessage = "Audio session failed: \(error.localizedDescription)"
            persistSharedPayload(hostBundleId: hostBundleId)
            return
        }
        #endif

        let now = Date().timeIntervalSince1970
        let lease = FlowLease(acquiredAt: now, ttlSeconds: ttlSeconds)
        self.activeLease = lease
        self.remainingLeaseSeconds = ttlSeconds
        self.sessionState = .warming
        self.lastErrorMessage = nil
        persistSharedPayload(hostBundleId: hostBundleId)
        postDarwinNotification(named: FlowDarwinNotification.leaseUpdated)
        startLeaseCountdownTimer()
    }

    /// Ends the warm microphone lease, stops `AVAudioEngine`, and ends the Live Activity.
    public func stopWarmAudioLease() {
        leaseTimer?.invalidate()
        leaseTimer = nil
        activeLease = nil
        remainingLeaseSeconds = 0
        sessionState = .cold
        recordedSamples.removeAll(keepingCapacity: false)

        #if canImport(AVFAudio)
        if isTapInstalled {
            audioEngine.inputNode.removeTap(onBus: 0)
            isTapInstalled = false
        }
        audioEngine.stop()
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        #endif

        persistSharedPayload()
        postDarwinNotification(named: FlowDarwinNotification.leaseUpdated)

        Task {
            await FlowLiveActivityCoordinator.shared.end()
        }
    }

    /// Starts capturing microphone PCM frames into the utterance buffer (triggered via Darwin IPC or in-app).
    public func beginUtteranceCapture() {
        let now = Date().timeIntervalSince1970
        if activeLease?.isWarm(at: now) != true {
            activeLease = FlowLease(acquiredAt: now, ttlSeconds: FlowLease.defaultTTLSeconds)
        }
        let latest = store.loadPayload()
        activeTone = latest.activeTone
        isWhisperModeEnabled = latest.whisperMode
        recordedSamples.removeAll(keepingCapacity: true)
        sessionState = .listening
        lastErrorMessage = nil
        persistSharedPayload()

        Task {
            await FlowLiveActivityCoordinator.shared.startOrUpdate(
                state: .listening,
                tone: activeTone,
                whisperMode: isWhisperModeEnabled,
                leaseExpiresAt: Date(timeIntervalSince1970: activeLease?.expiresAt ?? (now + 300))
            )
        }
    }

    /// Finalizes the captured utterance, applies +20 dB Whispering Mode software gain boost when enabled,
    /// encodes a 16 kHz mono PCM16 WAV, and uploads via `TranscriberRouter`.
    public func finishUtteranceAndTranscribe() async {
        guard sessionState == .listening else { return }
        sessionState = .transcribing
        persistSharedPayload()

        let rawSamples = recordedSamples
        recordedSamples.removeAll(keepingCapacity: false)

        let processedSamples: [Float]
        if isWhisperModeEnabled {
            processedSamples = WhisperGainBooster.boostPCMFloat32(
                rawSamples,
                decibels: WhisperGainBooster.defaultDecibels
            )
        } else {
            processedSamples = rawSamples
        }

        let wavData = Self.encodePCM16WAV(samples: processedSamples, sampleRate: sampleRate)
        let payload = store.loadPayload()
        let dictionary = store.loadPersonalDictionary()
        let snippets = AppGroupSettingsStore.shared.loadSnippets()

        let options = FlowRequestOptions(
            tone: payload.activeTone,
            stripFillers: true,
            backtrack: true,
            lists: true,
            whisperMode: isWhisperModeEnabled,
            selectedText: payload.selectedText,
            snippets: snippets,
            vocabulary: dictionary
        )

        let router = buildTranscriberRouter()
        do {
            let output = try await router.transcribe(audioData: wavData, options: options)
            var nextPayload = store.loadPayload()
            nextPayload.state = .readyToInsert
            nextPayload.seq += 1
            nextPayload.transcript = output.text
            nextPayload.errorMessage = nil
            nextPayload.updatedAt = Date().timeIntervalSince1970
            store.savePayload(nextPayload)

            self.sessionState = .readyToInsert
            self.lastTranscript = output.text
            self.lastTranscriptionSource = output.source
            postDarwinNotification(named: FlowDarwinNotification.transcriptReady)
        } catch {
            var failedPayload = store.loadPayload()
            failedPayload.state = .failed
            failedPayload.errorMessage = error.localizedDescription
            failedPayload.updatedAt = Date().timeIntervalSince1970
            store.savePayload(failedPayload)

            self.sessionState = .failed
            self.lastErrorMessage = error.localizedDescription
        }
    }

    public func handleScenePhaseChange(_ phase: ScenePhase) {
        if phase == .active, let lease = activeLease {
            let now = Date().timeIntervalSince1970
            remainingLeaseSeconds = lease.remainingSeconds(at: now)
            if !lease.isWarm(at: now) {
                stopWarmAudioLease()
            }
        }
    }

    // MARK: - Audio Engine Tap & WAV Encoding

    #if canImport(AVFAudio)
    private func installAudioEngineTapIfNeeded() {
        guard !isTapInstalled else {
            if !audioEngine.isRunning {
                try? audioEngine.start()
            }
            return
        }
        let inputNode = audioEngine.inputNode
        let format = inputNode.outputFormat(forBus: 0)
        inputNode.installTap(onBus: 0, bufferSize: 4_096, format: format) { [weak self] buffer, _ in
            guard let channelData = buffer.floatChannelData?[0] else { return }
            let frameCount = Int(buffer.frameLength)
            let chunk = Array(UnsafeBufferPointer(start: channelData, count: frameCount))
            Task { @MainActor [weak self] in
                guard let self, self.sessionState == .listening else { return }
                self.recordedSamples.append(contentsOf: chunk)
            }
        }
        isTapInstalled = true
        try? audioEngine.start()
    }
    #endif

    /// Encode normalized `Float` PCM samples into a canonical 44-byte RIFF/WAVE 16-bit mono payload.
    public static func encodePCM16WAV(samples: [Float], sampleRate: Int = 16_000) -> Data {
        let effectiveSamples = samples.isEmpty ? [Float](repeating: 0, count: 160) : samples
        let int16Samples: [Int16] = effectiveSamples.map { sample in
            let clamped = max(-1.0, min(1.0, sample))
            return Int16((clamped * Float(Int16.max)).rounded())
        }

        let numChannels: UInt16 = 1
        let bitsPerSample: UInt16 = 16
        let byteRate = UInt32(sampleRate) * UInt32(numChannels) * UInt32(bitsPerSample / 8)
        let blockAlign = numChannels * (bitsPerSample / 8)
        let dataSize = UInt32(int16Samples.count * MemoryLayout<Int16>.size)
        let chunkSize = 36 + dataSize

        var data = Data(capacity: Int(44 + dataSize))
        data.append("RIFF".data(using: .ascii)!)
        data.append(contentsOf: withUnsafeBytes(of: chunkSize.littleEndian) { Array($0) })
        data.append("WAVE".data(using: .ascii)!)
        data.append("fmt ".data(using: .ascii)!)
        let subchunk1Size: UInt32 = 16
        let audioFormat: UInt16 = 1 // PCM
        let sampleRate32 = UInt32(sampleRate)
        data.append(contentsOf: withUnsafeBytes(of: subchunk1Size.littleEndian) { Array($0) })
        data.append(contentsOf: withUnsafeBytes(of: audioFormat.littleEndian) { Array($0) })
        data.append(contentsOf: withUnsafeBytes(of: numChannels.littleEndian) { Array($0) })
        data.append(contentsOf: withUnsafeBytes(of: sampleRate32.littleEndian) { Array($0) })
        data.append(contentsOf: withUnsafeBytes(of: byteRate.littleEndian) { Array($0) })
        data.append(contentsOf: withUnsafeBytes(of: blockAlign.littleEndian) { Array($0) })
        data.append(contentsOf: withUnsafeBytes(of: bitsPerSample.littleEndian) { Array($0) })
        data.append("data".data(using: .ascii)!)
        data.append(contentsOf: withUnsafeBytes(of: dataSize.littleEndian) { Array($0) })

        for s in int16Samples {
            data.append(contentsOf: withUnsafeBytes(of: s.littleEndian) { Array($0) })
        }
        return data
    }

    private func buildTranscriberRouter() -> TranscriberRouter {
        let baseURL = AppGroupSettingsStore.shared.loadGatewayURL()
        let token = KeychainTokenStore.shared.loadToken() ?? ""
        let gateway = GatewayTranscriptionClient(baseURL: baseURL, deviceToken: token)
        let fallback = LocalFallbackTranscriber(adapters: [AppleSFSpeechAdapter()])
        return TranscriberRouter(
            primary: gateway,
            fallback: fallback,
            isOffline: { token.isEmpty }
        )
    }

    // MARK: - Lease Countdown & Darwin IPC

    private func startLeaseCountdownTimer() {
        leaseTimer?.invalidate()
        leaseTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self else { return }
                let now = Date().timeIntervalSince1970
                guard let lease = self.activeLease, lease.isWarm(at: now) else {
                    self.stopWarmAudioLease()
                    return
                }
                self.remainingLeaseSeconds = lease.remainingSeconds(at: now)
            }
        }
    }

    private func persistSharedPayload(hostBundleId: String? = nil) {
        var payload = store.loadPayload()
        payload.state = sessionState
        payload.lease = activeLease
        payload.activeTone = activeTone
        payload.whisperMode = isWhisperModeEnabled
        if let hostBundleId {
            payload.hostBundleId = hostBundleId
        }
        payload.updatedAt = Date().timeIntervalSince1970
        store.savePayload(payload)
    }

    private func registerDarwinNotificationObservers() {
        let center = CFNotificationCenterGetDarwinNotifyCenter()
        let observer = Unmanaged.passUnretained(self).toOpaque()

        CFNotificationCenterAddObserver(
            center,
            observer,
            { _, _, name, _, _ in
                guard let rawName = name?.rawValue as String? else { return }
                Task { @MainActor in
                    if rawName == FlowDarwinNotification.startRecording {
                        FlowAudioSessionManager.shared.beginUtteranceCapture()
                    } else if rawName == FlowDarwinNotification.stopRecording {
                        await FlowAudioSessionManager.shared.finishUtteranceAndTranscribe()
                    }
                }
            },
            FlowDarwinNotification.startRecording as CFString,
            nil,
            .deliverImmediately
        )

        CFNotificationCenterAddObserver(
            center,
            observer,
            { _, _, name, _, _ in
                guard let rawName = name?.rawValue as String? else { return }
                Task { @MainActor in
                    if rawName == FlowDarwinNotification.stopRecording {
                        await FlowAudioSessionManager.shared.finishUtteranceAndTranscribe()
                    }
                }
            },
            FlowDarwinNotification.stopRecording as CFString,
            nil,
            .deliverImmediately
        )
    }

    private func postDarwinNotification(named name: String) {
        let center = CFNotificationCenterGetDarwinNotifyCenter()
        CFNotificationCenterPostNotification(center, CFNotificationName(name as CFString), nil, nil, true)
    }
}

// MARK: - App Intents (Action Button, Back Tap, Control Center & Siri Shortcuts)

#if canImport(AppIntents)
public struct StartFlowSessionIntent: AppIntent {
    public static var title: LocalizedStringResource = "Start WordInk Flow Session"
    public static var description = IntentDescription(
        "Warms the background microphone lease for 5 minutes so you can dictate in any app from the WordInk Keyboard without switching apps."
    )
    public static var openAppWhenRun: Bool = true

    @Parameter(title: "Whisper Mode (+20 dB)", default: false)
    public var whisperMode: Bool

    public init() {}

    @MainActor
    public func perform() async throws -> some IntentResult {
        FlowAudioSessionManager.shared.isWhisperModeEnabled = whisperMode
        await FlowAudioSessionManager.shared.startWarmAudioLease(
            ttlSeconds: FlowLease.defaultTTLSeconds
        )
        await FlowLiveActivityCoordinator.shared.startOrUpdate(
            state: .warming,
            tone: FlowAudioSessionManager.shared.activeTone,
            whisperMode: whisperMode,
            leaseExpiresAt: Date().addingTimeInterval(FlowLease.defaultTTLSeconds)
        )
        return .result()
    }
}

public struct StopFlowSessionIntent: AppIntent {
    public static var title: LocalizedStringResource = "Stop WordInk Flow Session"
    public static var description = IntentDescription(
        "Immediately ends the background microphone lease and dismisses the WordInk Live Activity."
    )

    public init() {}

    @MainActor
    public func perform() async throws -> some IntentResult {
        FlowAudioSessionManager.shared.stopWarmAudioLease()
        return .result()
    }
}
#endif
