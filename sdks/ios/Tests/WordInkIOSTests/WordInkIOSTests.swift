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
}
