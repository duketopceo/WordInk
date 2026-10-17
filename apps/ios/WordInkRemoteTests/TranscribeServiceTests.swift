import XCTest
@testable import WordInkRemote

final class TranscribeServiceTests: XCTestCase {
    private let base = URL(string: "https://gw.ts.net")!

    override func setUp() { MockProtocol.reset() }

    private func makeFile() throws -> URL {
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("svc-clip.m4a")
        try Data("audio".utf8).write(to: file)
        return file
    }

    func testTranscribeUsesInjectedEndpointAndToken() async throws {
        MockProtocol.handler = { req in
            XCTAssertEqual(req.url?.absoluteString, "https://gw.ts.net/v1/audio/transcriptions")
            XCTAssertEqual(req.value(forHTTPHeaderField: "Authorization"), "Bearer wdk_t")
            return stubResponse(req.url!, status: 200, body: #"{"text":"hi"}"#)
        }
        let svc = TranscribeService(
            client: .mocked(),
            endpoint: { self.base },
            token: { "wdk_t" }
        )
        let text = try await svc.transcribe(fileURL: makeFile())
        XCTAssertEqual(text, "hi")
    }

    func testMissingEndpointThrowsInvalidEndpoint() async {
        let svc = TranscribeService(endpoint: { nil }, token: { "wdk_t" })
        do {
            _ = try await svc.transcribe(fileURL: makeFile())
            XCTFail("expected invalidEndpoint")
        } catch let error as GatewayError {
            guard case .invalidEndpoint = error else {
                return XCTFail("expected invalidEndpoint, got \(error)")
            }
        } catch {
            XCTFail("expected GatewayError, got \(error)")
        }
    }

    func testMissingTokenThrowsAuth() async {
        let svc = TranscribeService(endpoint: { self.base }, token: { nil })
        do {
            _ = try await svc.transcribe(fileURL: makeFile())
            XCTFail("expected auth")
        } catch let error as GatewayError {
            XCTAssertEqual(error, .auth)
        } catch {
            XCTFail("expected GatewayError, got \(error)")
        }
    }
}
