import Foundation

/// Transcript history, capped and persisted as JSON in Application Support (F2).
final class HistoryStore: ObservableObject {
    struct Entry: Codable, Equatable, Identifiable {
        let id: UUID
        let text: String
        let date: Date
    }

    static let shared = HistoryStore()
    static let cap = 20

    @Published private(set) var entries: [Entry] = []
    private let fileURL: URL

    init(directory: URL? = nil) {
        let dir = directory ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        fileURL = dir.appendingPathComponent("wordink-history.json")
        load()
    }

    func append(_ text: String) {
        entries.insert(Entry(id: UUID(), text: text, date: Date()), at: 0)
        if entries.count > Self.cap { entries = Array(entries.prefix(Self.cap)) }
        save()
    }

    private func load() {
        guard let data = try? Data(contentsOf: fileURL),
              let decoded = try? JSONDecoder().decode([Entry].self, from: data) else { return }
        entries = decoded
    }

    private func save() {
        try? FileManager.default.createDirectory(
            at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        guard let data = try? JSONEncoder().encode(entries) else { return }
        try? data.write(to: fileURL, options: .atomic)
    }
}
