// swift-tools-version: 5.9

import PackageDescription
import Foundation

let packageRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
let binetteTargetDebug = URL(fileURLWithPath: packageRoot)
    .appendingPathComponent("../../target/debug")
    .standardized
    .path

let package = Package(
    name: "BinetteSwiftProbes",
    products: [
        .library(
            name: "BinetteSwiftProbes",
            targets: ["BinetteSwiftProbes"]
        ),
    ],
    targets: [
        .target(
            name: "CBinette",
            linkerSettings: [
                .unsafeFlags([
                    "-L", binetteTargetDebug,
                    "-lbinette",
                    "-Xlinker", "-rpath",
                    "-Xlinker", binetteTargetDebug,
                ]),
            ]
        ),
        .target(
            name: "BinetteSwiftProbes",
            dependencies: ["CBinette"]
        ),
        .testTarget(
            name: "BinetteSwiftProbesTests",
            dependencies: ["BinetteSwiftProbes", "CBinette"]
        ),
    ]
)
