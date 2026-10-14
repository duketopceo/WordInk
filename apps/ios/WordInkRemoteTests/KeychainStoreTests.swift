import XCTest
@testable import WordInkRemote

final class KeychainStoreTests: XCTestCase {
    override func setUp() { KeychainStore.deleteToken() }
    override func tearDown() { KeychainStore.deleteToken() }

    func testRoundTripAndRotation() {
        XCTAssertNil(KeychainStore.loadToken())
        XCTAssertTrue(KeychainStore.saveToken("wdk_one"))
        XCTAssertEqual(KeychainStore.loadToken(), "wdk_one")
        XCTAssertTrue(KeychainStore.saveToken("wdk_two"))
        XCTAssertEqual(KeychainStore.loadToken(), "wdk_two")
        XCTAssertTrue(KeychainStore.deleteToken())
        XCTAssertNil(KeychainStore.loadToken())
    }
}
