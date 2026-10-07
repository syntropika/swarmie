// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "MPAIClient",
    platforms: [.macOS(.v13), .iOS(.v16)],
    products: [.library(name: "MPAIClient", targets: ["MPAIClient"])],
    targets: [.target(name: "MPAIClient"), .testTarget(name: "MPAIClientTests", dependencies: ["MPAIClient"])]
)
