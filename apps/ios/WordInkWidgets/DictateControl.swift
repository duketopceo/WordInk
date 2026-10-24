import AppIntents
import SwiftUI
import WidgetKit

@available(iOS 18.0, *)
struct DictateControl: ControlWidget {
    var body: some ControlWidgetConfiguration {
        StaticControlConfiguration(kind: "com.duketopceo.wordink.dictate") {
            ControlWidgetButton(action: StartDictationIntent()) {
                Label("Dictate", systemImage: "mic.fill")
            }
        }
    }
}

@available(iOS 18.0, *)
@main
struct WordInkWidgetsBundle: WidgetBundle {
    var body: some Widget {
        DictateControl()
    }
}
