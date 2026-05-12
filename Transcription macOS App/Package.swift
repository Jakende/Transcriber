// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "TranscriptionMacOSApp",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .executable(name: "TranscriptionMacOSApp", targets: ["TranscriptionMacOSApp"])
    ],
    targets: [
        .executableTarget(
            name: "TranscriptionMacOSApp",
            exclude: [
                "Resources/__pycache__"
            ],
            resources: [
                .copy("Resources/transcribe_bulk.py")
            ]
        )
    ]
)
