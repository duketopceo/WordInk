import XCTest
@testable import WordInkIOS

final class WordInkIOSTests: XCTestCase {
    func testFlowLeaseWarmthAndExpiration() {
        let lease = FlowLease(acquiredAt: 1_000, ttlSeconds: 300)
        XCTAssertTrue(lease.isWarm(at: 1_000))
        XCTAssertTrue(lease.isWarm(at: 1_299.9))
        XCTAssertEqual(lease.remainingSeconds(at: 1_120), 180, accuracy: 0.001)
        XCTAssertFalse(lease.isWarm(at: 1_300))
        XCTAssertEqual(lease.remainingSeconds(at: 1_300), 0)
    }

    func testKeyboardFlowControllerOpensHostAppWhenColdOrExpired() {
        let store = InMemoryAppGroupStore()
        let controller = KeyboardFlowController(store: store)

        let action = controller.onMicTap(now: 500, tone: .casual, hostBundleId: "com.apple.MobileSMS")
        XCTAssertEqual(
            action,
            .openHostApp(URL(string: "wordink://flow/warm?tone=casual&bundleId=com.apple.MobileSMS")!)
        )
        XCTAssertEqual(store.loadPayload().state, .warming)
    }

    func testKeyboardFlowControllerStartsAndStopsRecordingWhenWarmAndPollsOnce() {
        let warmPayload = SharedFlowPayload(
            state: .warming,
            lease: FlowLease(acquiredAt: 1_000, ttlSeconds: 300),
            seq: 1,
            activeTone: .work,
            updatedAt: 1_000
        )
        let store = InMemoryAppGroupStore(initialPayload: warmPayload)
        let controller = KeyboardFlowController(store: store)

        let startAction = controller.onMicTap(now: 1_050, tone: .work)
        XCTAssertEqual(
            startAction,
            .startRecordingIPC(notificationName: FlowDarwinNotification.startRecording)
        )
        XCTAssertEqual(store.loadPayload().state, .listening)

        let stopAction = controller.onMicRelease(now: 1_055)
        XCTAssertEqual(
            stopAction,
            .stopRecordingIPC(notificationName: FlowDarwinNotification.stopRecording)
        )
        XCTAssertEqual(store.loadPayload().state, .transcribing)

        // Simulate Host App completing transcription and writing seq = 2
        var readyPayload = store.loadPayload()
        readyPayload.state = .readyToInsert
        readyPayload.seq = 2
        readyPayload.transcript = "Let us ship WordInk today."
        store.savePayload(readyPayload)

        let firstPoll = controller.pollInsertion(lastSeq: 1)
        XCTAssertEqual(firstPoll.textToInsert, "Let us ship WordInk today.")
        XCTAssertEqual(firstPoll.lastSeq, 2)

        // Subsequent poll with updated lastSeq must never insert the same utterance twice
        let secondPoll = controller.pollInsertion(lastSeq: firstPoll.lastSeq)
        XCTAssertNil(secondPoll.textToInsert)
        XCTAssertEqual(secondPoll.lastSeq, 2)
    }

    func testContextToneMapperMapsHostBundlesAndTraits() {
        XCTAssertEqual(ContextToneMapper.tone(forBundleId: "com.apple.MobileSMS"), .casual)
        XCTAssertEqual(ContextToneMapper.tone(forBundleId: "com.tinyspeck.chatlyio"), .work)
        XCTAssertEqual(ContextToneMapper.tone(forBundleId: "com.apple.mobilemail"), .email)
        XCTAssertEqual(ContextToneMapper.tone(forBundleId: "com.Termius"), .code)

        let codeTraits = KeyboardFieldTraits(isMonospacedOrCodeField: true)
        XCTAssertEqual(ContextToneMapper.tone(forBundleId: "com.apple.mobilenotes", traits: codeTraits), .code)
    }

    func testEditVocabularyLearnerPersistsCorrectionsWithin20SecondWindow() {
        let store = InMemoryAppGroupStore(initialDictionary: ["Hyprland"])
        let learner = EditVocabularyLearner(store: store)

        learner.recordInsertion("We deployed word ink on omarchy today.", at: 2_000)

        // User edits within 20 seconds (at t = 2_012)
        let diff = learner.observeContextBeforeInput("We deployed WordInk on Omarchy today.", at: 2_012)
        XCTAssertEqual(diff.addedTerms, ["WordInk", "Omarchy"])
        XCTAssertEqual(diff.replacements, ["word ink": "WordInk", "omarchy": "Omarchy"])
        XCTAssertEqual(store.loadPersonalDictionary(), ["Hyprland", "WordInk", "Omarchy"])

        // Edits after the 20-second window are ignored
        let expiredDiff = learner.observeContextBeforeInput("We deployed WordInkPro on Omarchy today.", at: 2_025)
        XCTAssertEqual(expiredDiff.addedTerms, [])
    }

    func testGatewayTranscriptionClientBuildsMultipartRequestWithBearerAndFlowOptions() throws {
        let client = GatewayTranscriptionClient(
            baseURL: URL(string: "https://gateway.wordink.io")!,
            deviceToken: "wdk_live_test_token_123",
            model: "whisper-large-v3-turbo"
        )
        let options = FlowRequestOptions(
            tone: .email,
            stripFillers: true,
            backtrack: true,
            lists: false,
            whisperMode: true,
            selectedText: "Let us meet tomorrow.",
            snippets: ["my calendly": "https://cal.com/luke"],
            vocabulary: ["WordInk", "Omarchy"]
        )
        let audioBytes = Data([0x52, 0x49, 0x46, 0x46, 0x01, 0x02])
        let request = try client.buildMultipartRequest(
            audioData: audioBytes,
            filename: "utterance.wav",
            mimeType: "audio/wav",
            options: options,
            boundary: "WordInkTestBoundary"
        )

        XCTAssertEqual(request.url?.absoluteString, "https://gateway.wordink.io/v1/audio/transcriptions")
        XCTAssertEqual(request.httpMethod, "POST")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer wdk_live_test_token_123")
        XCTAssertEqual(
            request.value(forHTTPHeaderField: "Content-Type"),
            "multipart/form-data; boundary=WordInkTestBoundary"
        )
        XCTAssertEqual(request.value(forHTTPHeaderField: "X-WordInk-Tone"), "email")
        XCTAssertEqual(request.value(forHTTPHeaderField: "X-WordInk-Strip-Fillers"), "true")
        XCTAssertEqual(request.value(forHTTPHeaderField: "X-WordInk-Backtrack"), "true")
        XCTAssertEqual(request.value(forHTTPHeaderField: "X-WordInk-Lists"), "false")

        let bodyString = String(decoding: request.httpBody ?? Data(), as: UTF8.self)
        XCTAssertTrue(bodyString.contains("name=\"model\"\r\n\r\nwhisper-large-v3-turbo"))
        XCTAssertTrue(bodyString.contains("name=\"tone\"\r\n\r\nemail"))
        XCTAssertTrue(bodyString.contains("name=\"stripFillers\"\r\n\r\ntrue"))
        XCTAssertTrue(bodyString.contains("name=\"backtrack\"\r\n\r\ntrue"))
        XCTAssertTrue(bodyString.contains("name=\"lists\"\r\n\r\nfalse"))
        XCTAssertTrue(bodyString.contains("name=\"whisperMode\"\r\n\r\ntrue"))
        XCTAssertTrue(bodyString.contains("name=\"selectedText\"\r\n\r\nLet us meet tomorrow."))
        XCTAssertTrue(bodyString.contains("name=\"prompt\"\r\n\r\nWordInk, Omarchy"))
        XCTAssertTrue(bodyString.contains("name=\"file\"; filename=\"utterance.wav\""))
    }

    func testTranscriberRouterFallsBackToLocalWhenOfflineOrGatewayReturns5xx() async throws {
        struct StubTransport: HTTPDataTransport {
            let statusCode: Int
            func perform(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
                let response = HTTPURLResponse(
                    url: request.url!,
                    statusCode: statusCode,
                    httpVersion: "HTTP/1.1",
                    headerFields: ["Content-Type": "application/json"]
                )!
                let payload = Data("{\"error\":{\"message\":\"upstream gateway failure\"}}".utf8)
                return (payload, response)
            }
        }

        let gatewayClient = GatewayTranscriptionClient(
            baseURL: URL(string: "http://127.0.0.1:8941")!,
            deviceToken: "wdk_test_fallback",
            transport: StubTransport(statusCode: 502)
        )
        let whisperAdapter = WhisperKitAdapter(
            modelVariant: "openai_whisper-base",
            isAvailable: { true },
            transcribeHandler: { _, _ in
                "um ship the release with my calendly"
            }
        )
        let fallback = LocalFallbackTranscriber(adapters: [whisperAdapter])
        let router = TranscriberRouter(primary: gatewayClient, fallback: fallback)

        let output = try await router.transcribe(
            audioData: Data([0x01, 0x02, 0x03]),
            options: FlowRequestOptions(
                tone: .work,
                stripFillers: true,
                snippets: ["my calendly": "https://cal.com/luke"]
            )
        )

        XCTAssertEqual(output.source, .onDeviceWhisperKit)
        XCTAssertEqual(output.text, "Ship the release with https://cal.com/luke.")

        // Also verify immediate offline routing without calling primary
        let offlineRouter = TranscriberRouter(
            primary: gatewayClient,
            fallback: fallback,
            isOffline: { true }
        )
        let offlineOutput = try await offlineRouter.transcribe(
            audioData: Data([0x01, 0x02]),
            options: FlowRequestOptions(tone: .casual, stripFillers: true)
        )
        XCTAssertEqual(offlineOutput.source, .onDeviceWhisperKit)
        XCTAssertEqual(offlineOutput.text, "ship the release with my calendly")
    }

    func testWhisperGainBoosterApplies20dBLinearGainAndSoftKneeLimiting() {
        let linear = WhisperGainBooster.linearGain(forDecibels: 20.0)
        XCTAssertEqual(linear, 10.0, accuracy: 0.0001)

        // Quiet whisper sample (0.04) scales by 10x -> 0.40 (below 0.85 soft-knee threshold)
        // Loud sample (0.50) scales by 10x -> 5.0, soft-knee compressed into (0.85, 1.0] without clipping
        let input: [Float] = [0.0, 0.04, -0.05, 0.50, -0.80]
        let boosted = WhisperGainBooster.boostPCMFloat32(input, decibels: 20.0)

        XCTAssertEqual(boosted[0], 0.0, accuracy: 0.0001)
        XCTAssertEqual(boosted[1], 0.40, accuracy: 0.0001)
        XCTAssertEqual(boosted[2], -0.50, accuracy: 0.0001)
        XCTAssertGreaterThan(boosted[3], 0.85)
        XCTAssertLessThanOrEqual(boosted[3], 1.0)
        XCTAssertLessThan(boosted[4], -0.85)
        XCTAssertGreaterThanOrEqual(boosted[4], -1.0)
    }
}
