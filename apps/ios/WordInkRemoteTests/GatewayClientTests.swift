import XCTest
@testable import WordInkRemote

/// URLProtocol stub the GatewayClient tests inject via URLSessionConfiguration.
final class MockProtocol: URLProtocol {
    static var handler: ((URLRequest) throws -> (HTTPURLResponse, Data))?
    static var lastRequest: URLRequest?

    static func reset() {
        handler = nil
        lastRequest = nil
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        MockProtocol.lastRequest = request
        guard let handler = MockProtocol.handler else {
            client?.urlProtocol(self, didFailWithError: URLError(.unknown))
            return
        }
        do {
            let (response, data) = try handler(request)
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: data)
            client?.urlProtocolDidFinishLoading(self)
        } catch {
            client?.urlProtocol(self, didFailWithError: error)
        }
    }

    override func stopLoading() {}
}

extension GatewayClient {
    static func mocked() -> GatewayClient {
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockProtocol.self]
        return GatewayClient(session: URLSession(configuration: config))
    }
}

func stubResponse(_ url: URL, status: Int, body: String = "") -> (HTTPURLResponse, Data) {
    (HTTPURLResponse(url: url, statusCode: status, httpVersion: nil, headerFields: nil)!, Data(body.utf8))
}

final class GatewayClientTests: XCTestCase {
    private let base = URL(string: "https://gw.ts.net")!

    override func setUp() { MockProtocol.reset() }

    func testValidateSendsBearerAndSucceedsOn200() async throws {
        MockProtocol.handler = { req in
            XCTAssertEqual(req.url?.path, "/v1/models")
            XCTAssertEqual(req.value(forHTTPHeaderField: "Authorization"), "Bearer wdk_abc")
            return stubResponse(req.url!, status: 200, body: #"{"object":"list","data":[]}"#)
        }
        try await GatewayClient.mocked().validate(baseURL: base, token: "wdk_abc")
    }

    func testValidate401MapsToAuth() async {
        MockProtocol.handler = { req in stubResponse(req.url!, status: 401) }
        do {
            try await GatewayClient.mocked().validate(baseURL: base, token: "wdk_bad")
            XCTFail("expected auth error")
        } catch let error as GatewayError {
            XCTAssertEqual(error, .auth)
        }
    }

    func testValidateTransportFailureMapsToUnreachable() async {
        MockProtocol.handler = { _ in throw URLError(.timedOut) }
        do {
            try await GatewayClient.mocked().validate(baseURL: base, token: "wdk_abc")
            XCTFail("expected unreachable")
        } catch let error as GatewayError {
            XCTAssertEqual(error, .unreachable)
        }
    }

    func testValidate502MapsToServer() async {
        MockProtocol.handler = { req in stubResponse(req.url!, status: 502) }
        do {
            try await GatewayClient.mocked().validate(baseURL: base, token: "wdk_abc")
            XCTFail("expected server error")
        } catch let error as GatewayError {
            XCTAssertEqual(error, .server(502))
        }
    }

    func testTranscribeParsesTextField() async throws {
        MockProtocol.handler = { req in
            XCTAssertEqual(req.url?.path, "/v1/audio/transcriptions")
            XCTAssertTrue(req.value(forHTTPHeaderField: "Content-Type")?.contains("multipart/form-data") == true)
            let body = String(decoding: req.httpBody ?? Data(), as: UTF8.self)
            XCTAssertTrue(body.contains("name=\"file\""))
            XCTAssertTrue(body.contains("name=\"model\""))
            XCTAssertTrue(body.contains("whisper-1"))
            XCTAssertTrue(body.contains("fake-audio"))
            return stubResponse(req.url!, status: 200, body: #"{"text":"hello world"}"#)
        }
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("clip.m4a")
        try Data("fake-audio".utf8).write(to: file)
        let text = try await GatewayClient.mocked().transcribe(fileURL: file, baseURL: base, token: "wdk_abc")
        XCTAssertEqual(text, "hello world")
    }

    func testTranscribeMalformedJsonMapsToDecoding() async {
        MockProtocol.handler = { req in stubResponse(req.url!, status: 200, body: "not-json") }
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("clip.m4a")
        try? Data("x".utf8).write(to: file)
        do {
            _ = try await GatewayClient.mocked().transcribe(fileURL: file, baseURL: base, token: "wdk_abc")
            XCTFail("expected decoding error")
        } catch let error as GatewayError {
            XCTAssertEqual(error, .decoding)
        }
    }

    func testTranscribe403MapsToAuth() async {
        MockProtocol.handler = { req in stubResponse(req.url!, status: 403) }
        let file = FileManager.default.temporaryDirectory.appendingPathComponent("clip.m4a")
        try? Data("x".utf8).write(to: file)
        do {
            _ = try await GatewayClient.mocked().transcribe(fileURL: file, baseURL: base, token: "wdk_revoked")
            XCTFail("expected auth error")
        } catch let error as GatewayError {
            XCTAssertEqual(error, .auth)
        }
    }
}
