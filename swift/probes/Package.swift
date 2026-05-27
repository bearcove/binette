// swift-tools-version: 5.9

import PackageDescription

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
                    "-L", "../../target/debug",
                    "-lbinette",
                    "-Xlinker", "-rpath",
                    "-Xlinker", "../../target/debug",
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
