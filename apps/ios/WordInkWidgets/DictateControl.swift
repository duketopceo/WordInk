import AppIntents
import SwiftUI
import WidgetKit

struct DictateControl: ControlWidget {
    var body: some ControlWidgetConfiguration {
        StaticControlConfiguration(kind: "com.wordink.remote.dictate") {
            ControlButton(action: StartDictationIntent()) {
                Label("Dictate", systemImage: "mic.fill")
            }
        }
    }
}

@main
struct WordInkWidgetsBundle: WidgetBundle {
    var body: some Widget {
        DictateControl()
    }
}
