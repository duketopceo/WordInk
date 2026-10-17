import Foundation

/// Simplified representation of `UITextDocumentProxy` keyboard traits so `ContextToneMapper`
/// remains portable and testable on both iOS and macOS without importing UIKit.
public struct KeyboardFieldTraits: Equatable, Sendable {
    public var keyboardType: String?
    public var textContentType: String?
    public var autocorrectionDisabled: Bool
    public var isMonospacedOrCodeField: Bool

    public init(
        keyboardType: String? = nil,
        textContentType: String? = nil,
        autocorrectionDisabled: Bool = false,
        isMonospacedOrCodeField: Bool = false
    ) {
        self.keyboardType = keyboardType
        self.textContentType = textContentType
        self.autocorrectionDisabled = autocorrectionDisabled
        self.isMonospacedOrCodeField = isMonospacedOrCodeField
    }
}

/// Maps the host application's bundle identifier and active text-field traits to a `FlowTone` preset.
public enum ContextToneMapper {
    private static let casualBundles: Set<String> = [
        "com.apple.mobilesms",
        "net.whatsapp.whatsapp",
        "org.whispersystems.signal",
        "ph.telegra.telegraph",
        "com.atebits.tweetie2",
        "com.burbn.instagram",
        "com.hammerandchisel.discord"
    ]

    private static let workBundles: Set<String> = [
        "com.tinyspeck.chatlyio", // Slack
        "com.microsoft.skype.teams",
        "notion.id",
        "com.linear",
        "com.apple.mobilenotes",
        "com.google.docs"
    ]

    private static let emailBundles: Set<String> = [
        "com.apple.mobilemail",
        "com.google.gmail",
        "com.microsoft.office.outlook",
        "com.superhuman.mail",
        "com.readdle.smartemail"
    ]

    private static let codeBundles: Set<String> = [
        "com.termius",
        "com.termius.ios",
        "com.panic.prompt",
        "ish.app",
        "com.github.storm",
        "app.workingcopy"
    ]

    /// Resolve the appropriate `FlowTone` for a given host bundle ID and keyboard traits.
    public static func tone(
        forBundleId bundleId: String?,
        traits: KeyboardFieldTraits = KeyboardFieldTraits()
    ) -> FlowTone {
        if traits.isMonospacedOrCodeField {
            return .code
        }
        if let kb = traits.keyboardType?.lowercased(),
           kb == "url" || kb == "asciicapable" && traits.autocorrectionDisabled {
            return .code
        }

        if let rawBundle = bundleId?.trimmingCharacters(in: .whitespacesAndNewlines),
           !rawBundle.isEmpty {
            let lower = rawBundle.lowercased()
            if codeBundles.contains(lower) || lower.contains("termius") || lower.contains("terminal") || lower.contains("code") {
                return .code
            }
            if emailBundles.contains(lower) || lower.contains("mail") {
                return .email
            }
            if casualBundles.contains(lower) || lower.contains("mobilesms") || lower.contains("imessage") {
                return .casual
            }
            if workBundles.contains(lower) || lower.contains("slack") || lower.contains("chatlyio") {
                return .work
            }
        }

        if let contentType = traits.textContentType?.lowercased(), contentType.contains("email") {
            return .email
        }

        return .work
    }
}
