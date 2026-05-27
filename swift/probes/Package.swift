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
        .target(name: "BinetteSwiftProbes"),
        .testTarget(
            name: "BinetteSwiftProbesTests",
            dependencies: ["BinetteSwiftProbes"]
        ),
    ]
)
