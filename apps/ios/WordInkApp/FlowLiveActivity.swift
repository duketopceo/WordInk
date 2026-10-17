import Foundation
import SwiftUI
import WordInkIOS

#if canImport(ActivityKit) && canImport(WidgetKit)
import ActivityKit
import WidgetKit
#endif

/// Attributes for the Dynamic Island and Lock Screen Live Activity displayed while a 300-second
/// WordInk background microphone lease is warm or actively recording.
public struct FlowLiveActivityAttributes: Codable, Hashable, Sendable {
    public struct ContentState: Codable, Hashable, Sendable {
        public var sessionState: FlowSessionState
        public var activeTone: FlowTone
        public var whisperMode: Bool
        public var leaseExpiresAt: Date

        public init(
            sessionState: FlowSessionState,
            activeTone: FlowTone,
            whisperMode: Bool,
            leaseExpiresAt: Date
        ) {
            self.sessionState = sessionState
            self.activeTone = activeTone
            self.whisperMode = whisperMode
            self.leaseExpiresAt = leaseExpiresAt
        }
    }

    public var appName: String

    public init(appName: String = "WordInk Flow") {
        self.appName = appName
    }
}

#if canImport(ActivityKit)
extension FlowLiveActivityAttributes: ActivityAttributes {}
#endif

/// Coordinates starting, updating, and ending the `ActivityKit` Live Activity from `WordInkApp`.
@MainActor
public final class FlowLiveActivityCoordinator {
    public static let shared = FlowLiveActivityCoordinator()

    #if canImport(ActivityKit)
    private var currentActivity: Activity<FlowLiveActivityAttributes>?
    #endif

    private init() {}

    public func startOrUpdate(
        state: FlowSessionState,
        tone: FlowTone,
        whisperMode: Bool,
        leaseExpiresAt: Date
    ) async {
        #if canImport(ActivityKit)
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return }
        let contentState = FlowLiveActivityAttributes.ContentState(
            sessionState: state,
            activeTone: tone,
            whisperMode: whisperMode,
            leaseExpiresAt: leaseExpiresAt
        )
        let content = ActivityContent(state: contentState, staleDate: leaseExpiresAt)

        if let currentActivity {
            await currentActivity.update(content)
        } else {
            let attributes = FlowLiveActivityAttributes()
            currentActivity = try? Activity.request(
                attributes: attributes,
                content: content,
                pushType: nil
            )
        }
        #endif
    }

    public func end() async {
        #if canImport(ActivityKit)
        guard let currentActivity else { return }
        await currentActivity.end(nil, dismissalPolicy: .immediate)
        self.currentActivity = nil
        #endif
    }
}

/// Reusable Lock Screen / Notification Banner view for the WordInk Flow Live Activity.
public struct FlowLockScreenBannerView: View {
    public let state: FlowLiveActivityAttributes.ContentState

    public init(state: FlowLiveActivityAttributes.ContentState) {
        self.state = state
    }

    public var body: some View {
        HStack(spacing: 12) {
            Image(systemName: state.sessionState == .listening ? "waveform.circle.fill" : "mic.circle.fill")
                .font(.title2)
                .foregroundStyle(state.sessionState == .listening ? .red : .orange)

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(state.sessionState == .listening ? "Listening…" : "WordInk Mic Warm")
                        .font(.headline)
                    if state.whisperMode {
                        Text("Whisper +20 dB")
                            .font(.caption2.weight(.semibold))
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background( Color.purple.opacity(0.2), in: Capsule() )
                    }
                }
                Text("Tone: \(state.activeTone.rawValue.capitalized) • Lease ends \(state.leaseExpiresAt, style: .relative)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Link(destination: URL(string: "wordink://flow/activate?tone=\(state.activeTone.rawValue)")!) {
                Text("Refresh")
                    .font(.caption.weight(.semibold))
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(Color.accentColor.opacity(0.18), in: Capsule())
            }
        }
        .padding(14)
    }
}

#if canImport(ActivityKit) && canImport(WidgetKit)
/// Widget configuration rendering Lock Screen banner and Dynamic Island compact/expanded/minimal views.
public struct FlowLiveActivityWidget: Widget {
    public init() {}

    public var body: some WidgetConfiguration {
        ActivityConfiguration(for: FlowLiveActivityAttributes.self) { context in
            FlowLockScreenBannerView(state: context.state)
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Label(
                        context.state.sessionState == .listening ? "Recording" : "Warm",
                        systemImage: context.state.sessionState == .listening ? "waveform" : "mic.fill"
                    )
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(context.state.sessionState == .listening ? .red : .orange)
                }
                DynamicIslandExpandedRegion(.trailing) {
                    Text(context.state.activeTone.rawValue.uppercased())
                        .font(.caption2.monospaced().weight(.bold))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color.secondary.opacity(0.2), in: Capsule())
                }
                DynamicIslandExpandedRegion(.center) {
                    if context.state.whisperMode {
                        Label("Whisper Boost (+20 dB)", systemImage: "ear.and.waveform")
                            .font(.caption2)
                            .foregroundStyle(.purple)
                    } else {
                        Text("WordInk Flow Active")
                            .font(.caption)
                    }
                }
                DynamicIslandExpandedRegion(.bottom) {
                    HStack {
                        Text("Lease expires")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                        Spacer()
                        Text(context.state.leaseExpiresAt, style: .timer)
                            .font(.caption2.monospacedDigit())
                    }
                }
            } compactLeading: {
                Image(systemName: context.state.sessionState == .listening ? "waveform" : "mic.fill")
                    .foregroundStyle(context.state.sessionState == .listening ? .red : .orange)
            } compactTrailing: {
                Text(context.state.leaseExpiresAt, style: .timer)
                    .frame(width: 44)
                    .font(.caption2.monospacedDigit())
            } minimal: {
                Image(systemName: "mic.fill")
                    .foregroundStyle(.orange)
            }
            .widgetURL(URL(string: "wordink://flow/activate"))
        }
    }
}
#endif
