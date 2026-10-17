import SwiftUI

/// Bounded local transcript history (F2); tapping an entry re-copies it.
struct HistoryView: View {
    @ObservedObject var history: HistoryStore

    var body: some View {
        Group {
            if history.entries.isEmpty {
                Text("Nothing dictated yet")
                    .foregroundStyle(.secondary)
            } else {
                List(history.entries) { entry in
                    VStack(alignment: .leading, spacing: 4) {
                        Text(entry.text)
                            .lineLimit(2)
                        Text(entry.date, style: .relative)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .contentShape(Rectangle())
                    .onTapGesture {
                        UIPasteboard.general.string = entry.text
                    }
                }
            }
        }
        .navigationTitle("History")
    }
}
