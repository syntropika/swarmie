// swift-tools-version: 6.0
import Foundation
import PackageDescription

let hasFramework = FileManager.default.fileExists(atPath: URL(fileURLWithPath: #filePath).deletingLastPathComponent().appendingPathComponent("CSwarmie.xcframework").path)
let native: Target = hasFramework
    ? .binaryTarget(name: "CSwarmie", path: "CSwarmie.xcframework")
    : .target(name: "CSwarmie", linkerSettings: [.linkedLibrary("dl", .when(platforms: [.linux]))])
let package = Package(
    name: "SwarmieClient",
    platforms: [.macOS(.v13), .iOS(.v16)],
    products: [.library(name: "SwarmieClient", targets: ["SwarmieClient"])],
    targets: [
        native,
        .target(name: "SwarmieClient", dependencies: ["CSwarmie"],
                resources: hasFramework ? [] : [.copy("native")],
                swiftSettings: hasFramework ? [.define("SWARMIE_STATIC")] : [],
                linkerSettings: [.linkedFramework("Security", .when(platforms: [.macOS, .iOS])), .linkedFramework("SystemConfiguration", .when(platforms: [.macOS, .iOS]))]),
        .testTarget(name: "SwarmieClientTests", dependencies: ["SwarmieClient"])
    ]
)
