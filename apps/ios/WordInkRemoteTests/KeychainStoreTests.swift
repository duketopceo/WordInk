import XCTest
@testable import WordInkRemote

final class KeychainStoreTests: XCTestCase {
    override func setUp() { KeychainStore.deleteToken() }
    override func tearDown() { KeychainStore.deleteToken() }

    func testRoundTripAndRotation() {
        XCTAssertNil(KeychainStore.loadToken())
        XCTAssertEqual(KeychainStore.saveTokenStatus("wdk_one"), errSecSuccess)
        XCTAssertEqual(KeychainStore.loadToken(), "wdk_one")
        XCTAssertTrue(KeychainStore.saveToken("wdk_two"))
        XCTAssertEqual(KeychainStore.loadToken(), "wdk_two")
        XCTAssertTrue(KeychainStore.deleteToken())
        XCTAssertNil(KeychainStore.loadToken())
    }
}
