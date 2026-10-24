import XCTest
@testable import WordInkRemote

final class PairingLinkTests: XCTestCase {
    func testParsesMintedLink() throws {
        let url = URL(string: "wordink-remote://pair?url=https%3A%2F%2Fmax.ts.net&token=wdk_abc")!
        let (endpoint, token) = try PairingLink.parse(url)
        XCTAssertEqual(endpoint.absoluteString, "https://max.ts.net")
        XCTAssertEqual(token, "wdk_abc")
    }

    func testParsesLanHttpEndpoint() throws {
        let url = URL(string: "wordink-remote://pair?url=http%3A%2F%2F192.168.1.5%3A8941&token=wdk_x")!
        let (endpoint, token) = try PairingLink.parse(url)
        XCTAssertEqual(endpoint.absoluteString, "http://192.168.1.5:8941")
        XCTAssertEqual(token, "wdk_x")
    }

    func testRejectsWrongScheme() {
        let url = URL(string: "wordink://pair?url=https%3A%2F%2Fmax.ts.net&token=wdk_x")!
        XCTAssertThrowsError(try PairingLink.parse(url))
    }

    func testRejectsWrongAction() {
        let url = URL(string: "wordink-remote://dictate?url=https%3A%2F%2Fmax.ts.net&token=wdk_x")!
        XCTAssertThrowsError(try PairingLink.parse(url))
    }

    func testRejectsMissingToken() {
        let url = URL(string: "wordink-remote://pair?url=https%3A%2F%2Fmax.ts.net")!
        XCTAssertThrowsError(try PairingLink.parse(url))
    }

    func testRejectsMissingEndpoint() {
        let url = URL(string: "wordink-remote://pair?token=wdk_x")!
        XCTAssertThrowsError(try PairingLink.parse(url))
    }

    func testRejectsEndpointViolatingTransportPolicy() {
        // The same rule onboarding enforces — cleartext to a public host must die at parse.
        let url = URL(string: "wordink-remote://pair?url=http%3A%2F%2F203.0.113.5%3A8941&token=wdk_x")!
        XCTAssertThrowsError(try PairingLink.parse(url))
    }
}
