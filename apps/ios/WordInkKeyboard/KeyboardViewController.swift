import Foundation
import SwiftUI
import WordInkIOS

#if canImport(UIKit)
import UIKit

/// Universal iOS + iPadOS `UIInputViewController` Keyboard Extension (< 48 MB memory ceiling).
/// Delegates microphone capture and network upload to `WordInkApp` over `group.io.wordink.shared`
/// and Darwin notifications (`FlowDarwinNotification`), inserts cleaned text via
/// `textDocumentProxy.insertText(_:)`, captures `selectedText` for Command Mode, and runs the
/// 20-second post-insertion `EditVocabularyLearner`.
public final class KeyboardViewController: UIInputViewController {
    private let store: AppGroupStore = (UserDefaultsAppGroupStore() as AppGroupStore?) ?? InMemoryAppGroupStore()
    private lazy var flowController = KeyboardFlowController(store: store)
    private lazy var editLearner = EditVocabularyLearner(
        store: store,
        observationWindowSeconds: EditVocabularyLearner.defaultObservationWindowSeconds
    )
    private let viewModel = FlowKeyboardViewModel()
    private var hostingController: UIHostingController<FlowKeyboardBarView>?
    private var pollTimer: Timer?
    private var lastInsertedSeq: UInt64 = 0

    override public func viewDidLoad() {
        super.viewDidLoad()
        let initialPayload = store.loadPayload()
        lastInsertedSeq = initialPayload.seq
        viewModel.sessionState = initialPayload.state
        viewModel.activeTone = initialPayload.activeTone
        viewModel.whisperMode = initialPayload.whisperMode
        viewModel.needsInputModeSwitchKey = needsInputModeSwitchKey
        viewModel.hasFullAccess = hasFullAccess

        installKeyboardBarView()
        registerDarwinObservers()
        startFallbackPollTimer()
    }

    override public func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        refreshContextAndState()
    }

    override public func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        if viewModel.sessionState == .listening {
            handleMicRelease()
        }
    }

    override public func textWillChange(_ textInput: UITextInput?) {
        super.textWillChange(textInput)
        refreshSelectedTextForCommandMode()
    }

    override public func textDidChange(_ textInput: UITextInput?) {
        super.textDidChange(textInput)
        refreshSelectedTextForCommandMode()

        // 20-second post-insertion observation window for automatic Personal Dictionary learning
        if let contextBefore = textDocumentProxy.documentContextBeforeInput,
           !contextBefore.isEmpty {
            let now = Date().timeIntervalSince1970
            let diff = editLearner.observeContextBeforeInput(contextBefore, at: now)
            if !diff.addedTerms.isEmpty {
                viewModel.lastLearnedTerms = diff.addedTerms
            }
        }
    }

    // MARK: - Mic Hold-to-Talk & Tap-to-Toggle Actions

    public func handleMicPress() {
        let now = Date().timeIntervalSince1970
        let selected = textDocumentProxy.selectedText?.trimmingCharacters(in: .whitespacesAndNewlines)
        viewModel.selectedText = (selected?.isEmpty == false) ? selected : nil

        // Persist selectedText & whisperMode into the shared App Group payload before signaling the host app
        var prePayload = store.loadPayload()
        prePayload.selectedText = viewModel.selectedText
        prePayload.whisperMode = viewModel.whisperMode
        store.savePayload(prePayload)

        let action = flowController.onMicTap(
            now: now,
            tone: viewModel.activeTone,
            hostBundleId: viewModel.hostBundleId
        )

        switch action {
        case .openHostApp(let warmURL):
            viewModel.sessionState = .warming
            let activateURL = buildActivateDeepLink(from: warmURL)
            openHostAppURL(activateURL)
        case .startRecordingIPC(let notificationName):
            viewModel.sessionState = .listening
            postDarwinNotification(named: notificationName)
        case .stopRecordingIPC(let notificationName):
            viewModel.sessionState = .transcribing
            postDarwinNotification(named: notificationName)
        case .none:
            break
        }
    }

    public func handleMicRelease() {
        let now = Date().timeIntervalSince1970
        let action = flowController.onMicRelease(now: now)
        if case .stopRecordingIPC(let notificationName) = action {
            viewModel.sessionState = .transcribing
            postDarwinNotification(named: notificationName)
        }
    }

    public func handleMicTapToggle() {
        if viewModel.sessionState == .listening {
            handleMicRelease()
        } else {
            handleMicPress()
        }
    }

    // MARK: - Polling & Text Insertion

    public func checkAndInsertPendingTranscript() {
        let payload = store.loadPayload()
        viewModel.sessionState = payload.state
        if let lease = payload.lease {
            viewModel.isLeaseWarm = lease.isWarm(at: Date().timeIntervalSince1970)
        } else {
            viewModel.isLeaseWarm = false
        }

        let result = flowController.pollInsertion(lastSeq: lastInsertedSeq)
        guard let text = result.textToInsert, !text.isEmpty else {
            return
        }

        lastInsertedSeq = result.lastSeq
        textDocumentProxy.insertText(text)
        flowController.acknowledgeInsertion(seq: result.lastSeq, now: Date().timeIntervalSince1970)
        editLearner.recordInsertion(text, at: Date().timeIntervalSince1970)
    }

    private func refreshContextAndState() {
        viewModel.hasFullAccess = hasFullAccess
        viewModel.needsInputModeSwitchKey = needsInputModeSwitchKey
        refreshSelectedTextForCommandMode()

        let traits = KeyboardFieldTraits(
            keyboardType: String(describing: textDocumentProxy.keyboardType ?? .default),
            textContentType: textDocumentProxy.textContentType?.rawValue,
            autocorrectionDisabled: textDocumentProxy.autocorrectionType == .no,
            isMonospacedOrCodeField: false
        )
        if !viewModel.userDidOverrideTone {
            viewModel.activeTone = ContextToneMapper.tone(
                forBundleId: viewModel.hostBundleId,
                traits: traits
            )
        }
        checkAndInsertPendingTranscript()
    }

    private func refreshSelectedTextForCommandMode() {
        let selected = textDocumentProxy.selectedText?.trimmingCharacters(in: .whitespacesAndNewlines)
        viewModel.selectedText = (selected?.isEmpty == false) ? selected : nil
    }

    // MARK: - View Installation

    private func installKeyboardBarView() {
        let barView = FlowKeyboardBarView(
            viewModel: viewModel,
            onMicPress: { [weak self] in self?.handleMicPress() },
            onMicRelease: { [weak self] in self?.handleMicRelease() },
            onMicTapToggle: { [weak self] in self?.handleMicTapToggle() },
            onNextKeyboard: { [weak self] in self?.advanceToNextInputMode() },
            onSpace: { [weak self] in self?.textDocumentProxy.insertText(" ") },
            onReturn: { [weak self] in self?.textDocumentProxy.insertText("\n") },
            onDeleteBackward: { [weak self] in self?.textDocumentProxy.deleteBackward() },
            onSelectTone: { [weak self] tone in
                guard let self else { return }
                self.viewModel.userDidOverrideTone = true
                self.viewModel.activeTone = tone
                var payload = self.store.loadPayload()
                payload.activeTone = tone
                self.store.savePayload(payload)
            },
            onToggleWhisper: { [weak self] in
                guard let self else { return }
                self.viewModel.whisperMode.toggle()
                var payload = self.store.loadPayload()
                payload.whisperMode = self.viewModel.whisperMode
                self.store.savePayload(payload)
            }
        )

        let host = UIHostingController(rootView: barView)
        host.view.translatesAutoresizingMaskIntoConstraints = false
        host.view.backgroundColor = .clear
        addChild(host)
        view.addSubview(host.view)

        let heightConstraint = host.view.heightAnchor.constraint(
            equalToConstant: UIDevice.current.userInterfaceIdiom == .pad ? 88 : 64
        )
        heightConstraint.priority = .defaultHigh

        NSLayoutConstraint.activate([
            host.view.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            host.view.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            host.view.topAnchor.constraint(equalTo: view.topAnchor),
            host.view.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            heightConstraint
        ])

        host.didMove(toParent: self)
        self.hostingController = host
    }

    // MARK: - Deep Link & Darwin Notification Helpers

    private func buildActivateDeepLink(from warmURL: URL) -> URL {
        guard var components = URLComponents(url: warmURL, resolvingAgainstBaseURL: false) else {
            return URL(string: "wordink://flow/activate")!
        }
        components.path = "/activate"
        var items = components.queryItems ?? []
        if viewModel.whisperMode {
            items.append(URLQueryItem(name: "whisper", value: "1"))
        }
        components.queryItems = items
        return components.url ?? warmURL
    }

    private func openHostAppURL(_ url: URL) {
        var responder: UIResponder? = self
        let selector = sel_registerName("openURL:")
        while let current = responder {
            if current.responds(to: selector) {
                _ = current.perform(selector, with: url)
                return
            }
            responder = current.next
        }
    }

    private func registerDarwinObservers() {
        let center = CFNotificationCenterGetDarwinNotifyCenter()
        let observer = Unmanaged.passUnretained(self).toOpaque()

        for name in [FlowDarwinNotification.transcriptReady, FlowDarwinNotification.leaseUpdated] {
            CFNotificationCenterAddObserver(
                center,
                observer,
                { _, observerPtr, _, _, _ in
                    guard let observerPtr else { return }
                    let controller = Unmanaged<KeyboardViewController>.fromOpaque(observerPtr).takeUnretainedValue()
                    DispatchQueue.main.async {
                        controller.checkAndInsertPendingTranscript()
                    }
                },
                name as CFString,
                nil,
                .deliverImmediately
            )
        }
    }

    private func postDarwinNotification(named name: String) {
        let center = CFNotificationCenterGetDarwinNotifyCenter()
        CFNotificationCenterPostNotification(center, CFNotificationName(name as CFString), nil, nil, true)
    }

    private func startFallbackPollTimer() {
        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { [weak self] _ in
            self?.checkAndInsertPendingTranscript()
        }
    }
}
#endif
