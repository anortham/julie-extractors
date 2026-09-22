import Foundation

/// A value a request can carry.
protocol Payload {}

/// A payload that can be sent over the wire.
protocol Sendable: Payload {
    func encode() async throws -> Data?
}

/// HTTP verbs with their wire names.
enum HTTPMethod: String {
    case get = "GET", post = "POST", put, delete
}

/// A decoded node tree.
indirect enum Node {
    case leaf(Int), branch(left: Node, right: Node)
}

private enum Mode {
    case on, off
}

/// A request builder with stored, computed, observed, and lazy properties.
public final class Request: Sendable {
    var x, y: Double
    let width = 10, height = 20
    private var headers: [String: String] = [:]
    public private(set) var attempts = 0

    /// The number of bytes sent so far.
    var sent: Int = 0 {
        didSet { report(sent) }
    }

    var isValid: Bool {
        validate()
    }

    lazy var session: Session = {
        makeSession()
    }()

    var body: some Equatable { 1 }

    init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }

    deinit {
        teardown()
    }

    subscript(name: String) -> String? {
        headers[name]
    }

    func encode() async throws -> Data? {
        defer { attempts += 1 }
        let (head, tail) = (width, height)
        _ = (head, tail)
        let data = try await session.upload(self).validate().data
        return data
    }

    func names() -> [String] {
        headers.keys.sorted { $0 < $1 }
    }

    private func report(_ value: Int) {}
    private func validate() -> Bool { headers["Host"] != nil }
    private func makeSession() -> Session { Session(timeout: 30) }
    private func teardown() { headers.removeAll() }
}

extension Request: CustomStringConvertible {
    var description: String { "\(x),\(y)" }
}

private extension Request {
    func reset() { attempts = 0 }
}

extension Session: Payload {}
