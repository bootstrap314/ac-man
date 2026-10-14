// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "MideaControl",
    platforms: [.macOS(.v13)],
    dependencies: [.package(path: "generated/apple")],
    targets: [
        .executableTarget(
            name: "MideaControl",
            dependencies: [.product(name: "AcControl", package: "apple")],
            path: "native",
            linkerSettings: [.linkedFramework("Cocoa"), .linkedFramework("Security")]
        )
    ]
)
