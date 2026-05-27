// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "BinetteSwiftProbes",
    products: [
        .library(
            name: "BinetteSwiftProbes",
            targets: ["BinetteSwiftProbes"]
        ),
        .executable(
            name: "binette-swift-probes",
            targets: ["BinetteSwiftProbeDump"]
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
        .target(name: "BinetteSwiftProbes"),
        .executableTarget(
            name: "BinetteSwiftProbeDump",
            dependencies: ["BinetteSwiftProbes"]
        ),
        .testTarget(
            name: "BinetteSwiftProbesTests",
            dependencies: ["BinetteSwiftProbes", "CBinette"]
        ),
    ]
)
