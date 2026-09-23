/*
 License header block.
 */
import Foundation
@testable import App

struct Money: Equatable, Comparable {
    let cents: Int

    static func == (lhs: Money, rhs: Money) -> Bool { lhs.cents == rhs.cents }
    static func < (lhs: Money, rhs: Money) -> Bool { lhs.cents < rhs.cents }
    static prefix func - (m: Money) -> Money { Money(cents: -m.cents) }
}

infix operator <>: AdditionPrecedence

func <> (a: Money, b: Money) -> Money { a }

/// Stringifies an expression.
@freestanding(expression)
public macro stringify<T>(_ value: T) -> (T, String) = #externalMacro(module: "MacrosImpl", type: "StringifyMacro")

func useMacro() -> String {
    let (_, code) = #stringify(1 + 2)
    return code
}

final class Parser {
    init?(json: [String: Any]) { return nil }
    init!(raw: Int) {}
}

extension Array where Element: Equatable {
    /// Returns elements where the predicate holds.
    func matching(_ p: (Element) -> Bool) -> [Element] { filter(p) }
}

struct Store {
    // skip rows where id is nil
    func load<T>(_ t: T.Type) -> T? where T: Decodable { nil }
}

actor Worker: Sendable {}

/**
 Computes a total.
 */
func compute(_ x: Int) -> Int {
    /* Doubling cannot overflow for small inputs. */
    func double(_ y: Int) -> Int { y * 2 }
    return double(x)
}

enum Shape {
    case circle(radius: Double)
    case rect(width: Double, height: Double)
}

@main
struct Tool {
    @available(iOS 17, *)
    @discardableResult
    static func main() -> Int { 0 }

    static func make() -> Tool { Tool() }

    func build(session: Session) {
        session.request("u", interceptor: .retryPolicy(retryLimit: 3))
        let money: Money = .init(cents: 1)
        let tool = Self.make()
        let kind = [Money].self
    }
}
