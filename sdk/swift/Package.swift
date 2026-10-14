// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "SwarmieClient",
    platforms: [.macOS(.v13), .iOS(.v16)],
    products: [.library(name: "SwarmieClient", targets: ["SwarmieClient"])],
    targets: [.target(name: "SwarmieClient"), .testTarget(name: "SwarmieClientTests", dependencies: ["SwarmieClient"])]
)
