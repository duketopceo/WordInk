import XCTest
@testable import WordInkRemote

final class IntentTests: XCTestCase {
    override func setUp() { MockProtocol.reset() }

    func testIntentExecutionContexts() {
        // Foreground path opens the app; headless path never does (KTD6).
        XCTAssertTrue(StartDictationIntent.openAppWhenRun)
        XCTAssertFalse(TranscribeAudioIntent.openAppWhenRun)
    }

    func testPendingDictationArmsAndConsumesOnce() {
        let defaults = UserDefaults(suiteName: "IntentTests")!
        defer { defaults.removePersistentDomain(forName: "IntentTests") }
        XCTAssertFalse(PendingDictation.consume(in: defaults))
        PendingDictation.arm(in: defaults)
        XCTAssertTrue(PendingDictation.consume(in: defaults))
        XCTAssertFalse(PendingDictation.consume(in: defaults))
    }

    func testTranscribeAudioRunReturnsText() async throws {
        MockProtocol.handler = { req in
            stubResponse(req.url!, status: 200, body: #"{"text":"siri path"}"#)
        }
        let svc = TranscribeService(
            client: .mocked(),
            endpoint: { URL(string: "https://gw.ts.net") },
            token: { "wdk_t" }
        )
        let text = try await TranscribeAudioIntent.run(service: svc, data: Data("audio".utf8))
        XCTAssertEqual(text, "siri path")
    }

    func testTranscribeAudioRunPropagatesGatewayError() async {
        MockProtocol.handler = { _ in throw URLError(.cannotConnectToHost) }
        let svc = TranscribeService(
            client: .mocked(),
            endpoint: { URL(string: "https://gw.ts.net") },
            token: { "wdk_t" }
        )
        do {
            _ = try await TranscribeAudioIntent.run(service: svc, data: Data("x".utf8))
            XCTFail("expected unreachable")
        } catch let error as GatewayError {
            XCTAssertEqual(error, .unreachable)
        } catch {
            XCTFail("expected GatewayError, got \(error)")
        }
    }
}
