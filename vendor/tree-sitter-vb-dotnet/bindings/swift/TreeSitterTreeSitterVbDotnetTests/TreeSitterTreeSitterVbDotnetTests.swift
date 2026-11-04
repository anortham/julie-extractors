import XCTest
import SwiftTreeSitter
import TreeSitterTreeSitterVbDotnet

final class TreeSitterTreeSitterVbDotnetTests: XCTestCase {
    func testCanLoadGrammar() throws {
        let parser = Parser()
        let language = Language(language: tree_sitter_tree_sitter_vb_dotnet())
        XCTAssertNoThrow(try parser.setLanguage(language),
                         "Error loading TreeSitterVbDotnet grammar")
    }
}
