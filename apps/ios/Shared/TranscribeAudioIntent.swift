import AppIntents
import Foundation

/// Headless file -> transcript intent for Shortcuts (KTD6 background path).
/// iOS gives an intent no microphone, so headless capture is the Shortcuts
/// composition: Record Audio -> Transcribe Audio -> act on the result text.
struct TranscribeAudioIntent: AppIntent {
    static var title: LocalizedStringResource = "Transcribe Audio"
    static var openAppWhenRun: Bool = false

    // supportedContentTypes: on @Parameter is iOS 18+; the plain file
    // parameter keeps iOS 17 support. The gateway validates the audio anyway.
    @Parameter(title: "Audio file")
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
