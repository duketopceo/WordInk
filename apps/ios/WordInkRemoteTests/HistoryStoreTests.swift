import XCTest
@testable import WordInkRemote

final class HistoryStoreTests: XCTestCase {
    private var dir: URL!

    override func setUp() {
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("hs-\(UUID().uuidString)")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: dir)
    }

    func testAppendPersistsAndNewestFirst() {
        let store = HistoryStore(directory: dir)
        store.append("first")
        store.append("second")
        XCTAssertEqual(store.entries.map(\.text), ["second", "first"])

        let reloaded = HistoryStore(directory: dir)
        XCTAssertEqual(reloaded.entries.map(\.text), ["second", "first"])
    }

    func testCapDropsOldest() {
        let store = HistoryStore(directory: dir)
        for i in 0..<(HistoryStore.cap + 5) { store.append("t\(i)") }
        XCTAssertEqual(store.entries.count, HistoryStore.cap)
        XCTAssertEqual(store.entries.first?.text, "t\(HistoryStore.cap + 4)")
        XCTAssertEqual(store.entries.last?.text, "t5")
    }

    func testMalformedFileLoadsEmpty() {
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? Data("garbage".utf8).write(to: dir.appendingPathComponent("wordink-history.json"))
        let store = HistoryStore(directory: dir)
        XCTAssertTrue(store.entries.isEmpty)
        store.append("recovered")
        XCTAssertEqual(store.entries.map(\.text), ["recovered"])
    }
}
