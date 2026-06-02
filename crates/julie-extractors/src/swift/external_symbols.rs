use crate::base::{Symbol, SymbolKind, UnresolvedTarget};
use std::collections::HashSet;

#[derive(Debug, Default)]
pub(super) struct SwiftImportContext {
    modules: HashSet<String>,
}

impl SwiftImportContext {
    pub(super) fn from_symbols(symbols: &[Symbol]) -> Self {
        let modules = symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Import)
            .map(|symbol| symbol.name.clone())
            .collect();
        Self { modules }
    }

    fn imports(&self, module: &str) -> bool {
        self.modules.contains(module)
    }

    fn imports_any(&self, modules: &[&str]) -> bool {
        modules.iter().any(|module| self.imports(module))
    }
}

pub(super) fn is_external_inheritance_target(name: &str, imports: &SwiftImportContext) -> bool {
    is_swift_standard_protocol_or_type(name)
        || (imports.imports("SwiftUI") && is_swiftui_inheritance_target(name))
        || (imports.imports_any(&["AppKit", "UIKit"]) && is_cocoa_inheritance_target(name))
}

pub(super) fn is_external_call_target(
    target: &UnresolvedTarget,
    imports: &SwiftImportContext,
) -> bool {
    if let Some(receiver) = target.receiver.as_deref() {
        return is_external_receiver(receiver, imports);
    }

    let name = target.terminal_name.as_str();
    is_swift_standard_direct_call(name)
        || (imports.imports("SwiftUI") && is_swiftui_direct_call(name))
        || (imports.imports("Foundation") && is_foundation_direct_call(name))
        || (imports.imports_any(&["AppKit", "UIKit"]) && is_cocoa_direct_call(name))
        || (imports.imports_any(&["XCTest", "Testing", "Quick", "Nimble"])
            && is_swift_test_framework_call(name))
}

fn is_external_receiver(receiver: &str, imports: &SwiftImportContext) -> bool {
    let root = receiver.split('.').next().unwrap_or(receiver);
    is_swift_standard_receiver(root)
        || (imports.imports("Foundation") && is_foundation_receiver(root))
        || (imports.imports("SwiftUI") && is_swiftui_receiver(root))
        || (imports.imports_any(&["AppKit", "UIKit"]) && is_cocoa_receiver(root))
        || (imports.imports("OSLog") && root == "Logger")
}

fn is_swift_standard_protocol_or_type(name: &str) -> bool {
    matches!(
        name,
        "Any"
            | "Array"
            | "Bool"
            | "CaseIterable"
            | "Codable"
            | "Collection"
            | "Comparable"
            | "CustomDebugStringConvertible"
            | "CustomStringConvertible"
            | "Decodable"
            | "Dictionary"
            | "Double"
            | "Encodable"
            | "Equatable"
            | "Error"
            | "Float"
            | "Hashable"
            | "Identifiable"
            | "Int"
            | "IteratorProtocol"
            | "LocalizedError"
            | "Never"
            | "Optional"
            | "RandomAccessCollection"
            | "RawRepresentable"
            | "Sequence"
            | "Sendable"
            | "Set"
            | "String"
            | "UInt"
            | "Void"
    )
}

fn is_swift_standard_direct_call(name: &str) -> bool {
    matches!(
        name,
        "Array"
            | "Bool"
            | "Double"
            | "Float"
            | "Int"
            | "Set"
            | "String"
            | "UInt"
            | "abs"
            | "assert"
            | "debugPrint"
            | "dump"
            | "fatalError"
            | "max"
            | "min"
            | "precondition"
            | "print"
            | "readLine"
            | "stride"
            | "type(of:)"
            | "zip"
    )
}

fn is_swift_standard_receiver(root: &str) -> bool {
    matches!(root, "Task" | "MainActor" | "Self")
}

fn is_swiftui_inheritance_target(name: &str) -> bool {
    matches!(name, "App" | "Scene" | "View" | "ViewModifier")
}

fn is_swiftui_direct_call(name: &str) -> bool {
    matches!(
        name,
        "AnyView"
            | "Binding"
            | "Button"
            | "Circle"
            | "Color"
            | "Divider"
            | "ForEach"
            | "Form"
            | "Group"
            | "HStack"
            | "Image"
            | "Label"
            | "LabeledContent"
            | "List"
            | "NavigationStack"
            | "Picker"
            | "ProgressView"
            | "Rectangle"
            | "RoundedRectangle"
            | "Section"
            | "Spacer"
            | "Text"
            | "TextEditor"
            | "TextField"
            | "Toggle"
            | "VStack"
            | "withAnimation"
    )
}

fn is_swiftui_receiver(root: &str) -> bool {
    matches!(
        root,
        "Animation" | "Color" | "Edge" | "Font" | "Image" | "Text"
    )
}

fn is_foundation_direct_call(name: &str) -> bool {
    matches!(
        name,
        "Bundle"
            | "Data"
            | "Date"
            | "DateFormatter"
            | "DispatchQueue"
            | "FileManager"
            | "JSONDecoder"
            | "JSONEncoder"
            | "Locale"
            | "NSError"
            | "NSNull"
            | "ProcessInfo"
            | "Timer"
            | "URL"
            | "URLRequest"
            | "UUID"
    )
}

fn is_foundation_receiver(root: &str) -> bool {
    matches!(
        root,
        "Bundle"
            | "DispatchQueue"
            | "FileManager"
            | "JSONSerialization"
            | "NotificationCenter"
            | "ProcessInfo"
            | "Timer"
            | "URLSession"
            | "UserDefaults"
    )
}

fn is_cocoa_inheritance_target(name: &str) -> bool {
    matches!(name, "NSObject" | "NSViewController" | "UIViewController")
}

fn is_cocoa_direct_call(name: &str) -> bool {
    matches!(
        name,
        "NSImage"
            | "NSMenu"
            | "NSMenuItem"
            | "NSRect"
            | "NSSize"
            | "NSView"
            | "UIColor"
            | "UIImage"
            | "UIView"
    )
}

fn is_cocoa_receiver(root: &str) -> bool {
    matches!(root, "NSApp" | "NSColor" | "UIScreen" | "UIView")
}

fn is_swift_test_framework_call(name: &str) -> bool {
    name.starts_with("XCTAssert")
        || matches!(
            name,
            "expect"
                | "fail"
                | "it"
                | "describe"
                | "context"
                | "beforeEach"
                | "afterEach"
                | "waitUntil"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_protocols_are_external_without_imports() {
        let imports = SwiftImportContext::default();
        assert!(is_external_inheritance_target("Codable", &imports));
        assert!(is_external_inheritance_target("Sendable", &imports));
        assert!(!is_external_inheritance_target(
            "ExternalProtocol",
            &imports
        ));
    }

    #[test]
    fn framework_calls_require_matching_imports() {
        let swiftui = context_with_imports(["SwiftUI"]);
        assert!(is_external_call_target(
            &UnresolvedTarget::simple("Text"),
            &swiftui
        ));

        let no_imports = SwiftImportContext::default();
        assert!(!is_external_call_target(
            &UnresolvedTarget::simple("Text"),
            &no_imports
        ));
    }

    fn context_with_imports<const N: usize>(modules: [&str; N]) -> SwiftImportContext {
        SwiftImportContext {
            modules: modules.into_iter().map(String::from).collect(),
        }
    }
}
