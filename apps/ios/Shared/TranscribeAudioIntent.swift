import AppIntents
import Foundation
import UniformTypeIdentifiers

/// Headless file -> transcript intent for Shortcuts (KTD6 background path).
/// iOS gives an intent no microphone, so headless capture is the Shortcuts
/// composition: Record Audio -> Transcribe Audio -> act on the result text.
struct TranscribeAudioIntent: AppIntent {
    static var title: LocalizedStringResource = "Transcribe Audio"
    static var openAppWhenRun: Bool = false

    @Parameter(title: "Audio file", supportedContentTypes: [.audio])
    var audioFile: IntentFile

    var service = TranscribeService()

    static func run(service: TranscribeService, data: Data) async throws -> String {
        let tmp = FileManager.default.temporaryDirectory
            .appendingPathComponent("intent-\(UUID().uuidString).m4a")
        try data.write(to: tmp)
        defer { try? FileManager.default.removeItem(at: tmp) }
        return try await service.transcribe(fileURL: tmp)
    }

    func perform() async throws -> some IntentResult & ReturnsValue<String> {
        .result(value: try await Self.run(service: service, data: audioFile.data))
    }
}
