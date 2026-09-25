//! Reproduction test suite demonstrating cross-language parity gaps relative to
//! Python's v3.6.1 and v3.6.2 contract updates.
//!
//! Each test case asserts CURRENT extractor behavior across TypeScript, JavaScript,
//! C++, and Ruby, substantiating the parity gaps identified during the Cross-Language
//! Parity Audit (v3.6.0..HEAD).
//!
//! All tests use in-memory `extract_canonical` and complete in <100ms.

use crate::base::{IdentifierKind, SymbolKind};
use crate::{ExtractionResults, extract_canonical};
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test"))
        .unwrap_or_else(|e| panic!("extraction failed for {path}: {e:?}"))
}

// ----------------------------------------------------------------------------
// Gap A: TypeScript Decorator Argument Stripping in Signatures
// ----------------------------------------------------------------------------
// In Python (commit ea793be3 / `python-decorator-args-v1`), decorator arguments
// are retained on a single line in signatures (e.g. `@bp.route("/users", methods=["GET"]) def get_users()`).
//
// In TypeScript (`crates/julie-extractors/src/typescript/helpers.rs:105-135`),
// `extract_single_decorator_name` only extracts the bare function identifier from
// a `call_expression` decorator, discarding all arguments. The signature is built
// as `@Route getUsers(): string` rather than `@Route('/api/v1/users', { auth: true }) getUsers(): string`.

#[test]
fn repro_ts_decorator_arguments_stripped_in_signatures() {
    let source = r#"
class ApiController {
    @Route('/api/v1/users', { auth: true })
    getUsers(): string {
        return "users";
    }
}
"#;
    let result = extract("controller.ts", source);
    let method = result
        .symbols
        .iter()
        .find(|s| s.name == "getUsers" && s.kind == SymbolKind::Method)
        .expect("getUsers method symbol found");

    let sig = method.signature.as_deref().expect("method signature present");

    // Verified behavior: Decorator arguments are retained in the signature.
    assert_eq!(
        sig,
        "@Route('/api/v1/users', { auth: true }) getUsers(): string"
    );
    assert!(
        sig.contains("/api/v1/users"),
        "Decorator arguments ('/api/v1/users') must be present in signature"
    );
    assert!(
        sig.contains("auth"),
        "Decorator options must be present in signature"
    );
}

// ----------------------------------------------------------------------------
// Gap B: JavaScript / TypeScript `instanceof` Identifier Classification
// ----------------------------------------------------------------------------
// In Python (commit ea793be3 / `python-isinstance-type-v1`), the class argument of
// `isinstance(x, MyClass)` is classified as `IdentifierKind::TypeUsage` instead of `VariableRef`.
//
// In JavaScript (`crates/julie-extractors/src/javascript/identifiers.rs:480-498`) and
// TypeScript, `binary_expression` with an `instanceof` operator falls through the
// value-read matcher to `_ => true`, causing `MyClass` in `x instanceof MyClass`
// to be emitted as `IdentifierKind::VariableRef` instead of `IdentifierKind::TypeUsage`.

#[test]
fn repro_js_instanceof_classified_as_variable_ref_instead_of_type_usage() {
    let source = r#"
function validate(x) {
    if (x instanceof CustomValidator) {
        return true;
    }
    return false;
}
"#;
    let result = extract("validate.js", source);
    let ids: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| i.name == "CustomValidator")
        .collect();

    assert!(
        !ids.is_empty(),
        "Expected at least one identifier for CustomValidator"
    );

    // Verified behavior: CustomValidator is classified as TypeUsage.
    assert_eq!(
        ids[0].kind,
        IdentifierKind::TypeUsage,
        "JS extraction classifies instanceof target as TypeUsage"
    );
}

#[test]
fn repro_ts_instanceof_classified_as_variable_ref_instead_of_type_usage() {
    let source = r#"
class ValidatorService {
    check(x: unknown): boolean {
        return x instanceof CustomValidator;
    }
}
"#;
    let result = extract("service.ts", source);
    let ids: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| i.name == "CustomValidator")
        .collect();

    assert!(
        !ids.is_empty(),
        "Expected identifier for CustomValidator in TS"
    );

    // Verified behavior: In TS, instanceof target is classified as TypeUsage.
    assert_eq!(
        ids[0].kind,
        IdentifierKind::TypeUsage,
        "TS extraction classifies instanceof target as TypeUsage"
    );
}

// ----------------------------------------------------------------------------
// Gap C: JavaScript Property Suppression When Getter/Setter Exists
// ----------------------------------------------------------------------------
// In Python (commit 77cd5314 / `python-property-init-v1`), a class-level `@property def x(self):`
// no longer suppresses the instance attribute assignment `self.x = value` in `__init__`. Both
// rows are emitted so consumers can see where the backing attribute is initialized.
//
// In JavaScript (`crates/julie-extractors/src/javascript/assignments.rs:177-181`),
// `extract_constructor_property` checks `symbols.iter().any(|symbol| symbol.parent_id == class_id && symbol.name == name)`.
// If the class defines a getter `get config()` or setter `set config(v)`, any constructor assignment
// `this.config = cfg` is silently suppressed and completely omitted from the symbols table.

#[test]
fn repro_js_property_suppression_when_getter_exists() {
    let source = r#"
class AppConfig {
    get config() {
        return this._config;
    }
    constructor(cfg) {
        this.config = cfg;
    }
}
"#;
    let result = extract("config.js", source);
    let config_symbols: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.name == "config")
        .collect();

    // Verified behavior: Both the getter AND the constructor-assigned property are emitted!
    assert_eq!(
        config_symbols.len(),
        2,
        "Expected 2 'config' symbols: the getter and the constructor property"
    );

    let ctor_prop = config_symbols
        .iter()
        .find(|s| {
            s.metadata
                .as_ref()
                .and_then(|m| m.get("isConstructorAssigned"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
        .expect("Constructor-assigned property found");

    assert_eq!(ctor_prop.kind, SymbolKind::Property);
}

// ----------------------------------------------------------------------------
// Gap D: C++ 0-Based and Angle-Bracketed Anonymous Union Naming
// ----------------------------------------------------------------------------
// In Python (commit 77cd5314 / `python-lambda-line-v1`), anonymous lambdas were updated
// from 0-based AST rows to 1-based line numbering (`lambda_{start_pos.row + 1}`) without
// punctuation or angle brackets.
//
// In C++ (`crates/julie-extractors/src/cpp/types.rs:187`), anonymous unions are named
// via `format!("<anonymous_union_{}>", node.start_position().row)`.
// This violates two conventions:
// 1. It uses 0-based AST row index (`node.start_position().row`), creating a 1-off mismatch
//    with the 1-based `start_line`.
// 2. It wraps the name in angle brackets `<...>`, which disrupt search tokenization.

#[test]
fn repro_cpp_anonymous_union_zero_based_and_bracketed_naming() {
    // Union starts on line 1 (0-based row 0)
    let source = "union { int integer_val; double float_val; };";
    let result = extract("union.cpp", source);

    let union_sym = result
        .symbols
        .iter()
        .find(|s| s.name.contains("anonymous_union"))
        .expect("anonymous union symbol found");

    // Verified behavior: 1-based row index (1) and without angle brackets.
    assert_eq!(
        union_sym.name, "anonymous_union_1",
        "C++ extractor names anonymous union with 1-based row and without angle brackets"
    );
    assert_eq!(
        union_sym.start_line, 1,
        "Start line is 1-based (line 1), matching 1-based name"
    );
    assert!(
        !union_sym.name.starts_with('<') && !union_sym.name.ends_with('>'),
        "Name is not wrapped in angle brackets, preserving search tokenization"
    );
}

#[test]
fn repro_cpp_anonymous_union_in_struct_zero_based_row_offset() {
    let source = r#"struct Packet {
    union {
        int id;
        char tag[4];
    };
};
"#;
    let result = extract("packet.cpp", source);

    let union_sym = result
        .symbols
        .iter()
        .find(|s| s.name.contains("anonymous_union"))
        .expect("anonymous union symbol found in Packet");

    // Line 1: `struct Packet {`
    // Line 2: `    union {` -> row index 1, start_line 2
    assert_eq!(union_sym.start_line, 2);
    // Verified behavior: uses 1-based row -> `anonymous_union_2`
    assert_eq!(union_sym.name, "anonymous_union_2");
}

// ----------------------------------------------------------------------------
// Gap E: Documentation Comment Gaps for Instance Properties
// ----------------------------------------------------------------------------
// In Python (commit 77cd5314 / `python-attribute-docs-v1`), `self.x = ...` instance attributes
// initialized in methods capture leading Sphinx `#:` comments or trailing docstrings in `doc_comment`.
//
// 1. In TypeScript, constructor-assigned instance properties (`this.x = value`) are completely
//    omitted as symbols (only explicit class fields and constructor parameter properties are
//    extracted). Consequently, any JSDoc comments preceding constructor assignments are dropped
//    because no symbol exists to receive them.
//
// 2. In JavaScript, trailing doc comments/strings directly following `this.x = value` (analogous
//    to PEP 257 docstrings supported for Python in `77cd5314`) are dropped.
//
// 3. Audit Verification Note: An earlier survey hypothesis suggested that setting `doc_comment: None`
//    in `javascript/assignments.rs:193` and `ruby/assignments.rs:90` caused preceding doc comments
//    to be dropped. Live testing disproved this: `BaseExtractor::create_symbol_from_span`
//    (creation_methods.rs:77) applies a fallback `options.doc_comment.or_else(|| self.find_doc_comment(node))`,
//    which successfully captures directly preceding doc comments for both JS and Ruby.

#[test]
fn repro_ts_constructor_assigned_properties_omitted_entirely() {
    let source = r#"
class ConnectionPool {
    constructor() {
        /** Maximum number of retry attempts */
        this.maxRetries = 5;
    }
}
"#;
    let result = extract("pool.ts", source);
    let prop = result
        .symbols
        .iter()
        .find(|s| s.name == "maxRetries")
        .expect("Constructor-assigned maxRetries property extracted in TS");

    // Verified behavior: TypeScript extracts constructor-assigned properties as Property symbols.
    assert_eq!(prop.kind, SymbolKind::Property);
    assert_eq!(
        prop.doc_comment.as_deref(),
        Some("/** Maximum number of retry attempts */")
    );
}

#[test]
fn repro_js_trailing_attribute_docstring_dropped() {
    let source = r#"
class ConnectionPool {
    constructor() {
        this.maxRetries = 5;
        /** Maximum number of retry attempts */
    }
}
"#;
    let result = extract("pool.js", source);
    let prop = result
        .symbols
        .iter()
        .find(|s| s.name == "maxRetries")
        .expect("maxRetries symbol found in JS");

    // Trailing doc comments (supported in Python PEP 257 via commit 77cd5314) are dropped in JS:
    assert_eq!(
        prop.doc_comment, None,
        "Trailing doc comment is not captured for constructor property in JS"
    );
}

#[test]
fn audit_verification_preceding_doc_comments_rescued_by_base_extractor_fallback() {
    // This test substantiates the forensic audit finding that BaseExtractor rescues
    // preceding doc comments despite SymbolOptions having doc_comment: None.
    let js_source = r#"
class ConnectionPool {
    constructor() {
        /** Maximum number of retry attempts */
        this.maxRetries = 5;
    }
}
"#;
    let js_result = extract("pool.js", js_source);
    let js_prop = js_result
        .symbols
        .iter()
        .find(|s| s.name == "maxRetries")
        .expect("maxRetries found in JS");
    assert_eq!(
        js_prop.doc_comment.as_deref(),
        Some("/** Maximum number of retry attempts */")
    );

    let rb_source = r#"
class PaymentGateway
  def initialize
    # Total transaction amount in cents
    @amount = 1000
  end
end
"#;
    let rb_result = extract("gateway.rb", rb_source);
    let rb_ivar = rb_result
        .symbols
        .iter()
        .find(|s| s.name == "@amount")
        .expect("@amount symbol found in Ruby");
    assert_eq!(
        rb_ivar.doc_comment.as_deref(),
        Some("# Total transaction amount in cents")
    );
}

// ----------------------------------------------------------------------------
// Codex Review Findings Repros
// ----------------------------------------------------------------------------

#[test]
fn repro_ts_constructor_property_does_not_duplicate_later_field_declaration() {
    let source = r#"
class Widget {
    constructor() {
        this.title = "hello";
    }
    title: string;
}
"#;
    let result = extract("widget.ts", source);
    let title_props: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.name == "title" && s.kind == SymbolKind::Property)
        .collect();
    assert_eq!(
        title_props.len(),
        1,
        "Expected exactly 1 title property symbol, found: {title_props:?}"
    );
}

#[test]
fn repro_qualified_instanceof_does_not_emit_duplicate_member_access_identifier() {
    let js_source = r#"
function check(x) {
    return x instanceof ns.Validator;
}
"#;
    let js_res = extract("check.js", js_source);
    let js_validators: Vec<_> = js_res
        .identifiers
        .iter()
        .filter(|i| i.name == "Validator")
        .collect();
    assert_eq!(
        js_validators.len(),
        1,
        "JS should emit exactly one identifier for Validator: {js_validators:?}"
    );
    assert_eq!(js_validators[0].kind, IdentifierKind::TypeUsage);

    let ts_source = r#"
function check(x: unknown) {
    return x instanceof ns.Validator;
}
"#;
    let ts_res = extract("check.ts", ts_source);
    let ts_validators: Vec<_> = ts_res
        .identifiers
        .iter()
        .filter(|i| i.name == "Validator")
        .collect();
    assert_eq!(
        ts_validators.len(),
        1,
        "TS should emit exactly one identifier for Validator: {ts_validators:?}"
    );
    assert_eq!(ts_validators[0].kind, IdentifierKind::TypeUsage);
}

