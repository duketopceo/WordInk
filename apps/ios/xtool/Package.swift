// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "WordInkRemote",
    platforms: [.iOS(.v17)],
    products: [
        .library(name: "WordInkRemote", targets: ["WordInkRemote"]),
        .library(name: "WordInkWidgets", targets: ["WordInkWidgets"]),
    ],
    targets: [
        // Shared/*.swift files are symlinked into both source dirs so each target
        // compiles them into its own module — same-module semantics as the xcodeproj.
        .target(name: "WordInkRemote", path: "Sources/WordInkRemote"),
        .target(name: "WordInkWidgets", path: "Sources/WordInkWidgets"),
    ],
    swiftLanguageVersions: [.v5]
)
