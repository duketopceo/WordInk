import AVFoundation
import SwiftUI

@MainActor
final class DictationViewModel: ObservableObject {
    enum State: Equatable {
        case ready
        case recording
        case uploading
        case done(String)
        case failed(GatewayError)
    }

    @Published private(set) var state: State = .ready

    private let recorder: ClipRecorder
    private let service: TranscribeService
    private let history: HistoryStore
    private let copy: (String) -> Void
    private let requestMic: () async -> Bool

    init(recorder: ClipRecorder = AudioRecorder(),
         service: TranscribeService = .init(),
         history: HistoryStore = .shared,
         copy: @escaping (String) -> Void = { UIPasteboard.general.string = $0 },
         requestMic: @escaping () async -> Bool = { await AVAudioApplication.requestRecordPermission() }) {
        self.recorder = recorder
        self.service = service
        self.history = history
        self.copy = copy
        self.requestMic = requestMic
    }

    func startRecording() {
        guard state != .recording && state != .uploading else { return }
        Task {
            guard await requestMic() else {
                state = .failed(.recording("microphone access denied — allow it in Settings"))
                return
            }
            do {
                try recorder.start()
                state = .recording
            } catch let error as GatewayError {
                state = .failed(error)
            } catch {
                state = .failed(.recording(error.localizedDescription))
            }
        }
    }

    func stopAndTranscribe() {
        guard state == .recording, let file = recorder.stop() else { return }
        state = .uploading
        Task {
            defer { ClipCleanup.delete(file) }
            do {
                let text = try await service.transcribe(fileURL: file)
                copy(text)
                history.append(text)
                state = .done(text)
            } catch {
                state = .failed(GatewayError.wrap(error))
            }
        }
    }

    func cancelRecording() {
        ClipCleanup.delete(recorder.stop())
        state = .ready
    }

    func reset() { state = .ready }
}
