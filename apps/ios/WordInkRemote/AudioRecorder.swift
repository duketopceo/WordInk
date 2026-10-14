import AVFoundation
import Foundation

/// Capture seam so DictationViewModel can run against a fake in tests.
protocol ClipRecorder {
    func start() throws
    func stop() -> URL?
}

/// Records AAC/M4A to a temp file via AVAudioRecorder. Foreground-only (KTD6).
final class AudioRecorder: ClipRecorder {
    private var recorder: AVAudioRecorder?
    private var fileURL: URL?

    func start() throws {
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker])
        try session.setActive(true)

        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("wordink-\(UUID().uuidString).m4a")
        let settings: [String: Any] = [
            AVFormatIDKey: kAudioFormatMPEG4AAC,
            AVSampleRateKey: 16000,
            AVNumberOfChannelsKey: 1,
            AVEncoderAudioQualityKey: AVAudioQuality.medium.rawValue,
        ]
        recorder = try AVAudioRecorder(url: url, settings: settings)
        guard recorder?.record() == true else {
            throw GatewayError.recording("could not start capture")
        }
        fileURL = url
    }

    func stop() -> URL? {
        recorder?.stop()
        try? AVAudioSession.sharedInstance().setActive(false)
        return fileURL
    }
}

enum ClipCleanup {
    static func delete(_ url: URL?) {
        guard let url else { return }
        try? FileManager.default.removeItem(at: url)
    }
}
