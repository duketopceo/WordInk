import Foundation
import SwiftUI
import WordInkIOS

/// Lightweight view model for `FlowKeyboardBarView` (< 48 MB Keyboard Extension memory ceiling).
public final class FlowKeyboardViewModel: ObservableObject {
    @Published public var sessionState: FlowSessionState = .cold
    @Published public var isLeaseWarm: Bool = false
    @Published public var activeTone: FlowTone = .work
    @Published public var userDidOverrideTone: Bool = false
    @Published public var whisperMode: Bool = false
    @Published public var selectedText: String?
    @Published public var hostBundleId: String?
    @Published public var needsInputModeSwitchKey: Bool = true
    @Published public var hasFullAccess: Bool = true
    @Published public var lastLearnedTerms: [String] = []

    public init() {}

    public var isCommandModeActive: Bool {
        guard let selectedText else { return false }
        return !selectedText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

/// Responsive SwiftUI toolbar embedded in `KeyboardViewController`:
/// - Compact pill toolbar on iPhone (`horizontalSizeClass == .compact`)
/// - Full-width shortcut + style preset bar on iPadOS (`horizontalSizeClass == .regular`)
/// Supports hold-to-talk, tap-to-toggle, Whispering mode (+20 dB) badge, and Command mode chip.
public struct FlowKeyboardBarView: View {
    @ObservedObject public var viewModel: FlowKeyboardViewModel
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @State private var isPressingHoldToTalk: Bool = false

    public var onMicPress: () -> Void
    public var onMicRelease: () -> Void
    public var onMicTapToggle: () -> Void
    public var onNextKeyboard: () -> Void
    public var onSpace: () -> Void
    public var onReturn: () -> Void
    public var onDeleteBackward: () -> Void
    public var onSelectTone: (FlowTone) -> Void
    public var onToggleWhisper: () -> Void

    public init(
        viewModel: FlowKeyboardViewModel,
        onMicPress: @escaping () -> Void = {},
        onMicRelease: @escaping () -> Void = {},
        onMicTapToggle: @escaping () -> Void = {},
        onNextKeyboard: @escaping () -> Void = {},
        onSpace: @escaping () -> Void = {},
        onReturn: @escaping () -> Void = {},
        onDeleteBackward: @escaping () -> Void = {},
        onSelectTone: @escaping (FlowTone) -> Void = { _ in },
        onToggleWhisper: @escaping () -> Void = {}
    ) {
        self.viewModel = viewModel
        self.onMicPress = onMicPress
        self.onMicRelease = onMicRelease
        self.onMicTapToggle = onMicTapToggle
        self.onNextKeyboard = onNextKeyboard
        self.onSpace = onSpace
        self.onReturn = onReturn
        self.onDeleteBackward = onDeleteBackward
        self.onSelectTone = onSelectTone
        self.onToggleWhisper = onToggleWhisper
    }

    public var body: some View {
        if !viewModel.hasFullAccess {
            fullAccessRequiredBanner
        } else if horizontalSizeClass == .regular {
            iPadFullWidthShortcutAndStyleBar
        } else {
            iPhoneCompactPillToolbar
        }
    }

    // MARK: - iPhone Compact Pill Toolbar

    private var iPhoneCompactPillToolbar: some View {
        HStack(spacing: 8) {
            if viewModel.needsInputModeSwitchKey {
                Button(action: onNextKeyboard) {
                    Image(systemName: "globe")
                        .font(.system(size: 16, weight: .medium))
                        .frame(width: 34, height: 36)
                        .background(Color.secondary.opacity(0.16), in: Circle())
                }
                .accessibilityLabel("Next Keyboard")
            }

            // Tone cycling chip
            Menu {
                ForEach(FlowTone.allCases, id: \.self) { tone in
                    Button {
                        onSelectTone(tone)
                    } label: {
                        Label(
                            tone.rawValue.capitalized,
                            systemImage: viewModel.activeTone == tone ? "checkmark" : "text.alignleft"
                        )
                    }
                }
            } label: {
                Text(viewModel.activeTone.rawValue.capitalized)
                    .font(.caption.weight(.semibold))
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(Color.secondary.opacity(0.16), in: Capsule())
            }

            if viewModel.isCommandModeActive {
                commandModeChip
            }

            whisperModeBadgeButton

            Spacer(minLength: 4)

            // Central Hold-to-Talk + Tap-to-Toggle Pill
            micPillButton

            Spacer(minLength: 4)

            Button(action: onSpace) {
                Image(systemName: "space")
                    .font(.system(size: 14, weight: .medium))
                    .frame(width: 34, height: 36)
                    .background(Color.secondary.opacity(0.16), in: RoundedRectangle(cornerRadius: 8))
            }
            .accessibilityLabel("Space")

            Button(action: onDeleteBackward) {
                Image(systemName: "delete.left")
                    .font(.system(size: 15, weight: .medium))
                    .frame(width: 34, height: 36)
                    .background(Color.secondary.opacity(0.16), in: RoundedRectangle(cornerRadius: 8))
            }
            .accessibilityLabel("Delete")
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
    }

    // MARK: - iPadOS Full-Width Shortcut + Style Preset Bar

    private var iPadFullWidthShortcutAndStyleBar: some View {
        VStack(spacing: 6) {
            HStack(spacing: 10) {
                if viewModel.needsInputModeSwitchKey {
                    Button(action: onNextKeyboard) {
                        Label("Keyboards", systemImage: "globe")
                            .font(.caption.weight(.medium))
                            .padding(.horizontal, 10)
                            .padding(.vertical, 6)
                            .background(Color.secondary.opacity(0.15), in: Capsule())
                    }
                }

                // Inline style preset bar on iPadOS
                HStack(spacing: 4) {
                    ForEach(FlowTone.allCases, id: \.self) { tone in
                        Button {
                            onSelectTone(tone)
                        } label: {
                            Text(tone.rawValue.capitalized)
                                .font(.caption.weight(viewModel.activeTone == tone ? .bold : .regular))
                                .padding(.horizontal, 10)
                                .padding(.vertical, 5)
                                .background(
                                    viewModel.activeTone == tone
                                        ? Color.accentColor.opacity(0.24)
                                        : Color.secondary.opacity(0.10),
                                    in: Capsule()
                                )
                        }
                        .buttonStyle(.plain)
                    }
                }

                if viewModel.isCommandModeActive {
                    commandModeChip
                }

                whisperModeBadgeButton

                Spacer()

                micPillButton

                Button(action: onSpace) {
                    Text("Space")
                        .font(.caption.weight(.medium))
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(Color.secondary.opacity(0.16), in: RoundedRectangle(cornerRadius: 8))
                }

                Button(action: onReturn) {
                    Image(systemName: "return")
                        .font(.system(size: 14, weight: .semibold))
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(Color.secondary.opacity(0.16), in: RoundedRectangle(cornerRadius: 8))
                }

                Button(action: onDeleteBackward) {
                    Image(systemName: "delete.left")
                        .font(.system(size: 14, weight: .semibold))
                        .padding(.horizontal, 12)
                        .padding(.vertical, 7)
                        .background(Color.secondary.opacity(0.16), in: RoundedRectangle(cornerRadius: 8))
                }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    // MARK: - Shared Controls (Mic Pill, Whispering Badge, Command Mode Chip)

    private var micPillButton: some View {
        let isListening = viewModel.sessionState == .listening
        let isTranscribing = viewModel.sessionState == .transcribing

        return HStack(spacing: 6) {
            Image(systemName: isListening ? "waveform" : (isTranscribing ? "ellipsis.Bubble" : "mic.fill"))
                .font(.system(size: 15, weight: .bold))
            Text(micButtonLabel)
                .font(.caption.weight(.bold))
        }
        .foregroundStyle(.white)
        .padding(.horizontal, 16)
        .padding(.vertical, 9)
        .background(
            isListening ? Color.red : (viewModel.isLeaseWarm ? Color.orange : Color.accentColor),
            in: Capsule()
        )
        .onTapGesture {
            onMicTapToggle()
        }
        .simultaneousGesture(
            LongPressGesture(minimumDuration: 0.25)
                .onEnded { _ in
                    isPressingHoldToTalk = true
                    onMicPress()
                }
        )
        .simultaneousGesture(
            DragGesture(minimumDistance: 0)
                .onEnded { _ in
                    if isPressingHoldToTalk {
                        isPressingHoldToTalk = false
                        onMicRelease()
                    }
                }
        )
        .accessibilityLabel("Flow Voice Dictation")
    }

    private var micButtonLabel: String {
        switch viewModel.sessionState {
        case .listening:
            return "Listening… Tap to Stop"
        case .transcribing:
            return "Cleaning…"
        case .warming:
            return "Hold or Tap to Talk"
        case .cold:
            return viewModel.isLeaseWarm ? "Flow Mic" : "Start Flow"
        case .readyToInsert:
            return "Inserted"
        case .failed:
            return "Retry Flow"
        }
    }

    private var whisperModeBadgeButton: some View {
        Button(action: onToggleWhisper) {
            HStack(spacing: 4) {
                Image(systemName: "ear.and.waveform")
                    .font(.caption2)
                Text(viewModel.whisperMode ? "Whisper +20 dB" : "Whisper")
                    .font(.caption2.weight(.semibold))
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .foregroundStyle(viewModel.whisperMode ? .purple : .secondary)
            .background(
                viewModel.whisperMode ? Color.purple.opacity(0.20) : Color.secondary.opacity(0.12),
                in: Capsule()
            )
        }
        .buttonStyle(.plain)
    }

    private var commandModeChip: some View {
        HStack(spacing: 4) {
            Image(systemName: "wand.and.stars")
                .font(.caption2)
            Text("Command Mode")
                .font(.caption2.weight(.semibold))
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 5)
        .foregroundStyle(.blue)
        .background(Color.blue.opacity(0.18), in: Capsule())
    }

    private var fullAccessRequiredBanner: some View {
        HStack(spacing: 10) {
            Image(systemName: "lock.shield")
                .foregroundStyle(.orange)
            Text("Enable \"Allow Full Access\" in Settings → Keyboards → WordInk to connect to the Flow session.")
                .font(.caption2)
            Spacer()
            if viewModel.needsInputModeSwitchKey {
                Button(action: onNextKeyboard) {
                    Image(systemName: "globe")
                }
            }
        }
        .padding(10)
    }
}
