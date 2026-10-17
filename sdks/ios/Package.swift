// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "WordInkIOS",
    platforms: [
        .iOS(.v17),
        .macOS(.v14)
    ],
    products: [
        .library(
            name: "WordInkIOS",
            targets: ["WordInkIOS"]
        )
    ],
    targets: [
        .target(
            name: "WordInkIOS",
            dependencies: [],
            path: "Sources/WordInkIOS"
        ),
        .testTarget(
            name: "WordInkIOSTests",
            dependencies: ["WordInkIOS"],
            path: "Tests/WordInkIOSTests"
        )
    ]
)
