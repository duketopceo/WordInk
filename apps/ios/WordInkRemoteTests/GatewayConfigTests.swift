import XCTest
@testable import WordInkRemote

final class GatewayConfigTests: XCTestCase {
    func testHttpsHostnameAccepted() throws {
        let url = try GatewayConfig.normalizedEndpoint("https://gw.tail-net.ts.net")
        XCTAssertEqual(url.scheme, "https")
        XCTAssertEqual(url.host, "gw.tail-net.ts.net")
    }

    func testHttpRfc1918Accepted() throws {
        XCTAssertNoThrow(try GatewayConfig.normalizedEndpoint("http://192.168.1.10:8941"))
        XCTAssertNoThrow(try GatewayConfig.normalizedEndpoint("http://10.0.0.2:8941"))
        XCTAssertNoThrow(try GatewayConfig.normalizedEndpoint("http://172.16.3.4:8941"))
    }

    func testHttpTailscaleCgnatAccepted() throws {
        XCTAssertNoThrow(try GatewayConfig.normalizedEndpoint("http://100.64.5.9:8941"))
        XCTAssertThrowsError(try GatewayConfig.normalizedEndpoint("http://100.128.0.1:8941"))
    }

    func testHttpPublicHostnameRejected() {
        XCTAssertThrowsError(try GatewayConfig.normalizedEndpoint("http://example.com"))
    }

    func testHttpPublicIpRejected() {
        XCTAssertThrowsError(try GatewayConfig.normalizedEndpoint("http://8.8.8.8"))
    }

    func testSchemelessLocalHostAccepted() throws {
        let url = try GatewayConfig.normalizedEndpoint("gw.local:8941")
        XCTAssertEqual(url.scheme, "http")
        XCTAssertEqual(url.host, "gw.local")
        XCTAssertEqual(url.port, 8941)
    }

    func testUserinfoRejected() {
        XCTAssertThrowsError(try GatewayConfig.normalizedEndpoint("https://user:pass@gw.ts.net"))
    }

    func testQueryAndFragmentRejected() {
        XCTAssertThrowsError(try GatewayConfig.normalizedEndpoint("https://gw.ts.net/?a=b"))
        XCTAssertThrowsError(try GatewayConfig.normalizedEndpoint("https://gw.ts.net/#x"))
    }

    func testPathRejected() {
        XCTAssertThrowsError(try GatewayConfig.normalizedEndpoint("https://gw.ts.net/api"))
    }

    func testHttpIpv6Rejected() {
        // IPv6 literals must not fall through the single-label-host rule into "LAN":
        // link-local, ULA, and global v6 all carry ":" — only ::1 is local.
        XCTAssertThrowsError(try GatewayConfig.normalizedEndpoint("http://[fe80::1]:8941"))
        XCTAssertThrowsError(try GatewayConfig.normalizedEndpoint("http://[fd00::1]:8941"))
        XCTAssertThrowsError(try GatewayConfig.normalizedEndpoint("http://[2606:4700::1]:8941"))
        XCTAssertNoThrow(try GatewayConfig.normalizedEndpoint("http://[::1]:8941"))
    }

    func testLoopbackAccepted() {
        XCTAssertNoThrow(try GatewayConfig.normalizedEndpoint("http://127.0.0.1:8941"))
        XCTAssertNoThrow(try GatewayConfig.normalizedEndpoint("http://localhost:8941"))
    }

    func testStoredEndpointRoundTrip() {
        let defaults = UserDefaults(suiteName: "GatewayConfigTests")!
        defer { defaults.removePersistentDomain(forName: "GatewayConfigTests") }
        XCTAssertNil(GatewayDefaults.storedEndpoint(in: defaults))
        let url = URL(string: "https://gw.ts.net")!
        defaults.set(url, forKey: GatewayDefaults.endpointKey)
        XCTAssertEqual(GatewayDefaults.storedEndpoint(in: defaults), url)
    }
}
