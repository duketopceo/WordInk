import Foundation

/// Result of comparing a dictated utterance with the user's immediate post-insertion edit.
public struct EditVocabularyDiff: Equatable, Sendable {
    public let addedTerms: [String]
    public let replacements: [String: String]

    public init(addedTerms: [String] = [], replacements: [String: String] = [:]) {
        self.addedTerms = addedTerms
        self.replacements = replacements
    }
}

/// Swift port of the post-insertion edit diff learner.
/// Detects when the user corrects a word in `documentContextBeforeInput` within a 20-second window
/// after dictation insertion and appends newly learned proper nouns / custom spellings to the
/// App Group personal dictionary.
public final class EditVocabularyLearner {
    /// Default observation window after insertion (20 seconds).
    public static let defaultObservationWindowSeconds: TimeInterval = 20

    private static let stopwords: Set<String> = [
        "a", "an", "the", "and", "or", "but", "if", "then", "else",
        "in", "on", "at", "to", "for", "of", "with", "by", "from",
        "is", "am", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "it", "its",
        "this", "that", "these", "those", "i", "you", "we", "they",
        "he", "she", "my", "your", "our", "their"
    ]

    private let store: AppGroupStore?
    private let observationWindowSeconds: TimeInterval

    private var lastInsertedText: String?
    private var lastInsertedAt: TimeInterval?

    public init(
        store: AppGroupStore? = nil,
        observationWindowSeconds: TimeInterval = EditVocabularyLearner.defaultObservationWindowSeconds
    ) {
        self.store = store
        self.observationWindowSeconds = observationWindowSeconds
    }

    /// Record that the keyboard extension just inserted `text` at timestamp `now`.
    public func recordInsertion(_ text: String, at now: TimeInterval) {
        self.lastInsertedText = text
        self.lastInsertedAt = now
    }

    /// Inspect `documentContextBeforeInput` at timestamp `now`.
    /// If within the 20-second observation window, computes the token diff, persists any newly
    /// learned terms into `AppGroupStore`, and returns the diff.
    @discardableResult
    public func observeContextBeforeInput(_ contextBeforeInput: String, at now: TimeInterval) -> EditVocabularyDiff {
        guard let dictated = lastInsertedText,
              let insertedAt = lastInsertedAt,
              now >= insertedAt,
              now - insertedAt <= observationWindowSeconds else {
            return EditVocabularyDiff()
        }

        let diff = Self.diff(dictated: dictated, edited: contextBeforeInput)
        if !diff.addedTerms.isEmpty, let store {
            var existing = store.loadPersonalDictionary()
            var seen = Set(existing.map { $0.lowercased() })
            for term in diff.addedTerms {
                let folded = term.lowercased()
                if !seen.contains(folded) {
                    seen.insert(folded)
                    existing.append(term)
                }
            }
            store.savePersonalDictionary(existing)
        }
        return diff
    }

    /// Pure token-diff learner matching `@wordink/server`'s `learnVocabularyFromEdit`.
    /// Ignores pure deletions and large rewrites (> 40% changed tokens).
    public static func diff(dictated: String, edited: String) -> EditVocabularyDiff {
        let dTokens = tokenize(dictated)
        let eTokens = tokenize(edited)
        guard !dTokens.isEmpty, !eTokens.isEmpty else {
            return EditVocabularyDiff()
        }

        let lcs = computeLCS(dTokens, eTokens)
        let maxLen = max(dTokens.count, eTokens.count)
        let minChanged = min(dTokens.count - lcs.count, eTokens.count - lcs.count)

        if lcs.isEmpty || (maxLen >= 3 && Double(minChanged) / Double(maxLen) > 0.4) {
            return EditVocabularyDiff()
        }

        var addedTerms: [String] = []
        var seenTerms = Set<String>()
        var replacements: [String: String] = [:]

        var dIdx = 0
        var eIdx = 0
        let anchors = lcs + [(d: dTokens.count, e: eTokens.count)]

        for anchor in anchors {
            let oldSpan = Array(dTokens[dIdx..<anchor.d])
            let newSpan = Array(eTokens[eIdx..<anchor.e])

            if !oldSpan.isEmpty && !newSpan.isEmpty && oldSpan.count <= 3 && newSpan.count <= 3 {
                let from = oldSpan.joined(separator: " ")
                let to = newSpan.joined(separator: " ")
                if from != to && isLearnableTerm(to: to, from: from) {
                    replacements[from] = to
                    let folded = to.lowercased()
                    if !seenTerms.contains(folded) {
                        seenTerms.insert(folded)
                        addedTerms.append(to)
                    }
                }
            }

            dIdx = anchor.d + 1
            eIdx = anchor.e + 1
        }

        return EditVocabularyDiff(addedTerms: addedTerms, replacements: replacements)
    }

    private static func tokenize(_ text: String) -> [String] {
        let trimSet = CharacterSet(charactersIn: ".,!?;:()\"'[]{}")
        return text
            .split(whereSeparator: { $0.isWhitespace })
            .map { String($0).trimmingCharacters(in: trimSet) }
            .filter { !$0.isEmpty }
    }

    private static func computeLCS(_ a: [String], _ b: [String]) -> [(d: Int, e: Int)] {
        let m = a.count
        let n = b.count
        var dp = Array(repeating: Array(repeating: 0, count: n + 1), count: m + 1)

        for i in stride(from: m - 1, through: 0, by: -1) {
            for j in stride(from: n - 1, through: 0, by: -1) {
                if a[i] == b[j] {
                    dp[i][j] = 1 + dp[i + 1][j + 1]
                } else {
                    dp[i][j] = max(dp[i + 1][j], dp[i][j + 1])
                }
            }
        }

        var pairs: [(d: Int, e: Int)] = []
        var i = 0
        var j = 0
        while i < m && j < n {
            if a[i] == b[j] {
                pairs.append((d: i, e: j))
                i += 1
                j += 1
            } else if dp[i + 1][j] >= dp[i][j + 1] {
                i += 1
            } else {
                j += 1
            }
        }
        return pairs
    }

    private static func isLearnableTerm(to: String, from: String) -> Bool {
        if stopwords.contains(to.lowercased()) {
            return false
        }
        let hasUpperOrSymbol = to.range(of: "[A-Z0-9_-]", options: .regularExpression) != nil
        // A respelling is a case-only change or a spacing change ("word ink" -> "wordink"), not an
        // arbitrary one-word substitution such as "meat" -> "meet".
        let toLower = to.lowercased()
        let fromLower = from.lowercased()
        let isRespelling = toLower == fromLower
            || fromLower.replacingOccurrences(of: " ", with: "") == toLower
        return hasUpperOrSymbol || isRespelling
    }
}
