// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "AcMan",
    platforms: [.macOS(.v13)],
    dependencies: [.package(path: "generated/apple")],
    targets: [
        .executableTarget(
            name: "AcMan",
            dependencies: [.product(name: "AcManCore", package: "apple")],
            path: "native",
            linkerSettings: [.linkedFramework("Cocoa"), .linkedFramework("Security")]
        )
    ]
)
