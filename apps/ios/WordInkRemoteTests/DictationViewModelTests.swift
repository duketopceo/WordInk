import XCTest
@testable import WordInkRemote

final class FakeRecorder: ClipRecorder {
    var file: URL?
    var startError: Error?

    func start() throws {
        if let startError { throw startError }
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("fake-\(UUID().uuidString).m4a")
        try? Data("audio".utf8).write(to: url)
        file = url
    }

    func stop() -> URL? { file }
}

@MainActor
final class DictationViewModelTests: XCTestCase {
    private var historyDir: URL!
    private var history: HistoryStore!
    private var recorder: FakeRecorder!
    private var copied: [String]!

    override func setUp() {
        MockProtocol.reset()
        historyDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("hist-\(UUID().uuidString)")
        history = HistoryStore(directory: historyDir)
        recorder = FakeRecorder()
        copied = []
    }

    private func makeVM(failUpload: Bool = false) -> DictationViewModel {
        MockProtocol.handler = { req in
            if failUpload { throw URLError(.timedOut) }
            return stubResponse(req.url!, status: 200, body: #"{"text":"hello"}"#)
        }
        let svc = TranscribeService(
            client: .mocked(),
            endpoint: { URL(string: "https://gw.ts.net") },
            token: { "wdk_t" }
        )
        return DictationViewModel(
            recorder: recorder,
            service: svc,
            history: history,
            copy: { self.copied.append($0) },
            requestMic: { true }
        )
    }

    private func waitFor(_ condition: @escaping () -> Bool) async {
        for _ in 0..<200 where !condition() {
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
    }

    func testHoldFlowCopiesTranscriptAndAppendsHistory() async {
        let vm = makeVM()
        vm.startRecording()
        await waitFor { vm.state == .recording }
        XCTAssertEqual(vm.state, .recording)

        vm.stopAndTranscribe()
        await waitFor {
            if case .done = vm.state { return true }
            return false
        }
        XCTAssertEqual(copied, ["hello"])
        XCTAssertEqual(history.entries.map(\.text), ["hello"])
    }

    func testGatewayFailureSurfacesTaxonomy() async {
        let vm = makeVM(failUpload: true)
        vm.startRecording()
        await waitFor { vm.state == .recording }
        vm.stopAndTranscribe()
        await waitFor {
            if case .failed = vm.state { return true }
            return false
        }
        XCTAssertEqual(vm.state, .failed(.unreachable))
        XCTAssertTrue(copied.isEmpty)
        XCTAssertTrue(history.entries.isEmpty)
    }

    func testMicDeniedFailsFast() async {
        let vm = DictationViewModel(
            recorder: recorder,
            service: .init(),
            history: history,
            copy: { self.copied.append($0) },
            requestMic: { false }
        )
        vm.startRecording()
        await waitFor {
            if case .failed = vm.state { return true }
            return false
        }
        guard case .failed(.recording) = vm.state else {
            return XCTFail("expected recording failure, got \(vm.state)")
        }
    }

    func testCancelDiscardsClip() async {
        let vm = makeVM()
        vm.startRecording()
        await waitFor { vm.state == .recording }
        let file = recorder.file
        vm.cancelRecording()
        XCTAssertEqual(vm.state, .ready)
        XCTAssertNotNil(file)
        XCTAssertFalse(FileManager.default.fileExists(atPath: file!.path))
    }
}
