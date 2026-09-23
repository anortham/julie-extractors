// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Alamofire",
    products: [
        .library(name: "Alamofire", targets: ["Alamofire"]),
    ],
    dependencies: [
        .package(url: "https://github.com/apple/swift-log.git", from: "1.5.0"),
    ],
    targets: [
        .target(name: "Alamofire", dependencies: [.product(name: "Logging", package: "swift-log")], path: "Source"),
        .testTarget(name: "AlamofireTests", dependencies: ["Alamofire"], path: "Tests"),
    ]
)
