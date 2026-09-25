# Comprehensive Cross-Language Parity & Correctness Audit Report

**Audit Range:** `v3.6.0..HEAD` (encompassing releases `v3.6.1` and `v3.6.2`)  
**Target Repository:** `julie-extractors` (`/home/murphy/source/julie-extractors`)  
**Date:** 2026-09-25  
**Auditor:** Teamwork Preview Audit Group (Verification & Implementation Worker)  
**Status:** Complete & Verified  

---

## 1. Executive Summary & Audit Context

### 1.1 Scope and Objectives
This audit evaluates all commits across `v3.6.0..HEAD`, encompassing patch releases **v3.6.1** and **v3.6.2**, which introduced a focused series of extraction bug fixes and semantic improvements to the Python extractor based on real-world dogfooding on corpora such as `flask`. 

The primary objectives of this audit are:
1. **R1: Python Correctness & Contract Verification:** Verify all 10 commits in `v3.6.0..HEAD` to confirm they adhere strictly to schema stability, contract versioning, and golden fixture standards, while identifying any latent edge cases or heuristics.
2. **R2: Cross-Language Parity Gap Analysis:** Systematically evaluate all 42 supported languages in `julie-extractors` (focusing on major general-purpose, systems, and scripting languages: Python, TypeScript, JavaScript, Ruby, Go, Rust, Java, C#, PHP, Kotlin, and C++) to determine whether analogous constructs exist and whether equivalent extraction behavior is upheld.
3. **R3: Test Demonstration & Quality Gate Verification:** Substantiate all identified parity gaps with dedicated in-memory reproduction tests executing in `<100ms`, verify full test suite cleanliness (`cargo test --lib`), and confirm zero data quality debts via `node scripts/language-data-quality-report.mjs --strict`.
4. **R4: Defect Specification & Implementation Roadmap:** Provide concrete, AST-level specifications of all identified defects, required closure steps, contract marker requirements, and an actionable implementation roadmap for subsequent development waves.

### 1.2 Summary of Findings
- **Python Correctness (R1):** All 10 commits in `v3.6.0..HEAD` strictly conform to project architectural standards. The SQLite schema (v7), Report schema (v3), and Identity Epoch (10) remain unchanged. All 10 behavioral changes are tracked by explicit contract markers in `EXTRACTION_CONTRACT_VERSION` in `crates/julie-extractors/src/lib.rs` and documented in `docs/contracts/extraction-output-changes.md`. Two minor implementation edge cases were identified for future hardening (a `"def "` string heuristic in property attribute deduplication, and non-simple pattern bindings in `binds_cls_locally`).
- **Cross-Language Gaps (R2 & R4):** While several extractors already follow consistent conventions (e.g. PHP and Kotlin return types, Java/C# annotation argument preservation), several high-impact parity gaps were identified and substantiated in **TypeScript**, **JavaScript**, **C++**, and **Ruby**:
  1. *TypeScript Decorator Argument Stripping:* Method and class signatures strip decorator arguments (e.g. `@Route('/api/v1', { auth: true })` degrades to bare `@Route`), whereas Python retains single-line collapsed arguments.
  2. *JavaScript & TypeScript `instanceof` Identifier Classification:* Type check expressions (`x instanceof MyClass`) classify `MyClass` as `IdentifierKind::VariableRef` instead of `IdentifierKind::TypeUsage`.
  3. *JavaScript Property Suppression:* Constructor assignments (`this.x = value`) are silently suppressed and dropped when a class getter or setter (`get x()`) exists on the class, violating the principle of property and assignment coexistence established in Python `v3.6.2`.
  4. *C++ Anonymous Union Naming:* Anonymous unions generate symbol names with 0-based AST rows and angle brackets (`<anonymous_union_0>`), violating search tokenization rules and 1-based coordinate consistency.
  5. *TypeScript Constructor Property Omission:* TypeScript omits constructor-assigned instance properties (`this.x = value`) entirely as symbols, preventing instance property documentation and symbol indexing.
  6. *JavaScript Trailing Docstring Omission:* JavaScript drops trailing docstrings/comments following constructor assignments (analogous to PEP 257 docstrings supported in Python `v3.6.2`).
- **Forensic Verification Finding:** A prior survey hypothesis claimed that JavaScript and Ruby dropped directly preceding doc comments on constructor properties due to `doc_comment: None` in local `SymbolOptions`. Live testing revealed this hypothesis to be incorrect: `BaseExtractor::create_symbol_from_span` contains an automatic fallback (`options.doc_comment.or_else(|| self.find_doc_comment(node))`) which rescues directly preceding doc comments. The true gaps in this domain are TypeScript's total omission of constructor-assigned property symbols and JavaScript's lack of support for trailing docstrings.
- **Verification Gates (R3):** All 9 reproduction and audit verification tests in `crates/julie-extractors/src/tests/repro_parity_gaps.rs` compile cleanly and execute in **0.01 seconds**. The full library test suite passes with **6,492 tests passing** (0 failing, 7 ignored). The strict data quality check reports **0 silent cells and 0 quality bar debts**.

---

## 2. Verification of Python Changes in `v3.6.0..HEAD`

### 2.1 Complete Commit Inventory (`v3.6.0..HEAD`)

Between tag `v3.6.0` and `HEAD`, 10 commits were authored. The table below documents each commit, files modified, extractor behavior alterations, and verification verdicts:

| # | Commit Hash | Date (UTC) | Category | Files Modified | Extractor Behavior Alteration | Verification Verdict |
|---|-------------|------------|----------|----------------|--------------------------------|----------------------|
| 1 | `548b29bf` | 2026-09-24 16:01 | Docs / Evidence | `docs/release-notes/v3.6.0.md`, `evidence/` | None. Records 3.6.0 publication evidence, asset sizes, and SHA-256 hashes. | Correct / Conforming |
| 2 | `fb5d40a5` | 2026-09-24 16:15 | Docs / Evidence | `evidence/` | None. Corrects typed-call total in 3.6.0 release evidence. | Correct / Conforming |
| 3 | `52b605fd` | 2026-09-24 18:26 | Extractor Code (3.6.1) | `crates/julie-extractors/src/python/assignments.rs`, `lib.rs`, `tests/` | Retains `self.` prefix in instance attribute signatures (`self.x = v`, `self.x: int = v`, `self.a, self.b = v`). Adds marker `python-instance-attribute-signature-v1`. | Correct / Conforming |
| 4 | `5787b88d` | 2026-09-25 00:52 | Docs / Evidence | `docs/release-notes/v3.6.1.md`, `evidence/` | None. Records 3.6.1 publication evidence, binaries, and checksums. | Correct / Conforming |
| 5 | `dc246243` | 2026-09-25 01:00 | Extractor Code (3.6.2) | `crates/julie-extractors/src/python/functions.rs`, `base/framework_structural_facts/python_web.rs`, `lib.rs`, `tests/` | 1. Python signatures use arrow `-> Ret` instead of `: Ret`.<br>2. `returnType` metadata holds clean type name without leading `: `.<br>3. Flask routes accept tuples `()` and sets `{}` for `methods=`. Adds markers `python-return-arrow-v1`, `flask-methods-tuple-v1`. | Correct / Conforming |
| 6 | `8dbe85a3` | 2026-09-25 01:33 | Extractor Code (3.6.2) | `crates/julie-extractors/src/python/helpers.rs`, `python/relationships.rs`, `lib.rs`, `tests/` | Restricts `cls(...)` and `cls.method()` resolution to the enclosing class to functions that take `cls` as a parameter. Local bindings prevent class resolution. Adds marker `python-cls-binding-v1`. | Correct / Conforming |
| 7 | `73c4c7ff` | 2026-09-25 01:36 | Docs / Notes | `docs/release-notes/v3.6.2.md` | None. Adjusts feature count in release notes. | Correct / Conforming |
| 8 | `ea793be3` | 2026-09-25 04:44 | Extractor Code (3.6.2) | `crates/julie-extractors/src/python/decorators.rs`, `functions.rs`, `types.rs`, `assignments.rs`, `identifiers.rs`, `lib.rs`, `tests/` | 1. Signatures include decorator arguments (max 100 chars, one line).<br>2. Variable annotations without value emit no trailing ` = `.<br>3. `isinstance` / `issubclass` class arguments emit `type_usage` identifiers. Adds markers `python-decorator-args-v1`, `python-annotation-only-v1`, `python-isinstance-type-v1`. | Correct / Conforming |
| 9 | `77cd5314` | 2026-09-25 05:50 | Extractor Code (3.6.2) | `crates/julie-extractors/src/python/functions.rs`, `assignments.rs`, `lib.rs`, `tests/` | 1. Lambdas named `lambda_{line}` using 1-based line indexing.<br>2. `self.x = ...` attributes capture `#:` Sphinx comments and PEP 257 docstrings.<br>3. Class `@property` methods no longer suppress `self.x = ...` instance attributes. Adds markers `python-lambda-line-v1`, `python-attribute-docs-v1`, `python-property-init-v1`. | Correct / Conforming |
| 10 | `08826563` | 2026-09-25 13:35 | Docs / Evidence | `docs/release-notes/v3.6.2.md`, `evidence/` | None. Records 3.6.2 publication evidence and release gate documentation. | Correct / Conforming |

---

### 2.2 Detailed Technical Analysis of the 9 Python Features

#### 1. One-Based Lambda Naming (`77cd5314`)
- **Source Location:** `crates/julie-extractors/src/python/functions.rs:139-140`
- **Contract Marker:** `python-lambda-line-v1`
- **Implementation:**
  ```rust
  let start_pos = node.start_position();
  let name = format!("lambda_{}", start_pos.row + 1);
  ```
- **Analysis:** Prior to commit `77cd5314`, lambdas were named using 0-based AST row indices (`start_pos.row`). This caused an off-by-one discrepancy between symbol names (e.g. `lambda_33`) and displayed 1-based source coordinates (`start_line: 34`). The fix increments the row by 1, aligning `name` directly with `start_line` and avoiding punctuation or angle brackets that degrade search tokenization.
- **Verdict:** Fully verified, correct, and backed by golden tests (`declarations`, `wave2_semantics`).

#### 2. Attribute Docstrings & Sphinx Comments (`77cd5314`)
- **Source Location:** `crates/julie-extractors/src/python/assignments.rs:87, 136-185, 283-287`
- **Contract Marker:** `python-attribute-docs-v1`
- **Implementation:**
  `attribute_doc_comment` accepts an `instance_attribute: bool` flag. When true, it permits searching within method blocks (`scope.kind() == "block"`). It checks preceding sibling comments for contiguous `#:` Sphinx documentation comments. If none exist, it inspects the immediate succeeding statement for an expression statement containing a string literal (PEP 257 docstring).
- **Analysis:** In Python, documenting instance attributes in `__init__` via Sphinx `#:` or trailing string docstrings is standard convention. Extending docstring extraction from module/class attributes to instance attributes restores internal consistency.
- **Verdict:** Fully verified and conforming.

#### 3. Property-Backed Attribute Preservation (`77cd5314`)
- **Source Location:** `crates/julie-extractors/src/python/assignments.rs:195-228`
- **Contract Marker:** `python-property-init-v1`
- **Implementation:**
  In `keep_first_attribute_declaration`, symbols were previously deduplicated against any existing class symbol with `SymbolKind::Property`. Since `@property def foo(self)` is classified as a `Property`, constructor assignments `self.foo = foo` were filtered out. The commit added a filter:
  ```rust
  .filter(|symbol| {
      !symbol.signature.as_deref().is_some_and(|sig| sig.contains("def "))
  })
  ```
- **Analysis:** Consumers like code-kb need to observe both the accessor declaration (`@property`) and the initialization point (`self.foo = ...`).
- **Edge Case / Caveat:** Checking `signature.contains("def ")` is a string heuristic. If a class variable literal contains `"def "` (e.g. `CODE = "def run(): pass"`), it will be filtered out, potentially allowing a duplicate instance attribute of the same name to avoid suppression. (See Section 2.4).
- **Verdict:** Functionally conforming; edge case documented for future AST hardening.

#### 4. Decorator Arguments in Signatures (`ea793be3`)
- **Source Location:** `crates/julie-extractors/src/python/decorators.rs:34-53`, `functions.rs:45`, `types.rs:97`
- **Contract Marker:** `python-decorator-args-v1`
- **Implementation:**
  Introduced `decorators::signature_prefix(&decorator_texts)`. Collapses multi-line decorator calls onto a single whitespace-normalized line and truncates decorators exceeding 100 characters with an ellipsis (`…`). The `decorators` array in symbol metadata retains the clean identifier name (e.g. `app.route`), preserving machine queryability.
- **Analysis:** In modern Python web frameworks (Flask, FastAPI, DRF), route decorators convey the essential semantics of endpoints (`@app.route("/items", methods=["GET"])`). Preserving arguments in signatures provides vital human context.
- **Verdict:** Fully verified, correct, and conforming.

#### 5. Annotation-Only Variable Declarations Without Trailing ` = ` (`ea793be3`)
- **Source Location:** `crates/julie-extractors/src/python/assignments.rs:66-70`
- **Contract Marker:** `python-annotation-only-v1`
- **Implementation:**
  ```rust
  let signature = if right.is_some() {
      format!("{target}{type_annotation} = {value}")
  } else {
      format!("{target}{type_annotation}")
  };
  ```
- **Analysis:** Previously, variable annotations without values (e.g. dataclass fields `timeout: float`) emitted `timeout: float = `. Omitting the trailing ` = ` cleans up symbols and prevents downstream presentation defects.
- **Verdict:** Fully verified bug fix.

#### 6. Runtime Type Checks as `type_usage` Identifiers (`ea793be3`)
- **Source Location:** `crates/julie-extractors/src/python/identifiers.rs:108-114, 159-166, 508-531`
- **Contract Marker:** `python-isinstance-type-v1`
- **Implementation:**
  `is_isinstance_class_argument` inspects whether an identifier or attribute node is the second argument of an `isinstance` or `issubclass` call (unwrapping a single tuple container if present). Non-builtin types in this position are emitted with `IdentifierKind::TypeUsage` instead of `VariableRef`.
- **Analysis:** Runtime type guards represent semantic type usages in Python. Classifying them as `TypeUsage` enables accurate cross-reference resolution.
- **Verdict:** Fully verified and conforming.

#### 7. `cls` Class Receiver Binding Scoping Rules (`8dbe85a3`)
- **Source Location:** `crates/julie-extractors/src/python/helpers.rs:81-152`, `relationships.rs:306`
- **Contract Marker:** `python-cls-binding-v1`
- **Implementation:**
  Replaced unconditional class resolution for `cls(...)` calls with `cls_names_enclosing_class`. Walks upward from the call:
  - Halts at `class_definition` (returning `false`).
  - At `function_definition`, checks if `cls` is in the parameter list. If so, returns `true`. If the function binds `cls` locally (`binds_cls_locally`), returns `false`. Otherwise, traverses to enclosing function scopes.
- **Analysis:** Prevents helper methods that assign `cls = self.factory_class` from falsely emitting constructor calls to the enclosing class.
- **Edge Case / Caveat:** `binds_cls_locally` checks simple assignments and imports (`import ... as cls`). It does not currently inspect destructuring assignments, for-loops, or walrus expressions. (See Section 2.4).
- **Verdict:** Major accuracy improvement for relationship extraction.

#### 8. Arrow Return Types & Flask Tuple/Set Methods (`dc246243`)
- **Source Location:** `crates/julie-extractors/src/python/functions.rs:27-35, 64`, `python_web.rs:1378-1409`
- **Contract Markers:** `python-return-arrow-v1`, `flask-methods-tuple-v1`
- **Implementation:**
  1. Python signatures emit `def func() -> Ret:` instead of `def func(): Ret`. In addition, `metadata["returnType"]` stores `"Ret"` without leading punctuation, aligning Python with Kotlin and PHP.
  2. Flask route parser matches tuple `b'('` and set `b'{'` literals in `methods=`, extracting all declared HTTP methods.
- **Analysis:** Eliminates invalid syntax artifacts in signatures and handles standard Flask idioms.
- **Verdict:** Fully verified and conforming.

#### 9. Preserving `self.` Receiver in Instance Attributes (`52b605fd`)
- **Source Location:** `crates/julie-extractors/src/python/assignments.rs:61-65, 275-279`
- **Contract Marker:** `python-instance-attribute-signature-v1`
- **Implementation:**
  Ensures instance attribute signatures retain the `self.` prefix (`self.x = v`, `self.x: int = v`, `self.a, self.b = v`).
- **Analysis:** Distinguishes instance attributes from class-level attributes, aligning Python with Ruby (`@x = v`) and JavaScript (`this.x = v`).
- **Verdict:** Fully verified and conforming.

---

### 2.3 Contract and Schema Conformance Audit

- **SQLite Schema Integrity:** Version remains **7** (`crates/julie-extractors/src/lib.rs` / `contracts/sqlite-schema-v7.md`). No tables, columns, indexes, or types were altered.
- **Report Schema Integrity:** Version remains **3** (`contracts/reports.md`). JSON output shapes and key names remain unchanged.
- **Identity Epoch:** Value remains **10** (`EXTRACTION_IDENTITY_EPOCH`).
- **Contract Version Markers:** All 10 markers are registered in `EXTRACTION_CONTRACT_VERSION` in `crates/julie-extractors/src/lib.rs` and verified by `crates/julie-extractors/src/tests/api_surface.rs`:
  ```
  python-instance-attribute-signature-v1
  python-return-arrow-v1
  flask-methods-tuple-v1
  python-cls-binding-v1
  python-decorator-args-v1
  python-annotation-only-v1
  python-isinstance-type-v1
  python-lambda-line-v1
  python-attribute-docs-v1
  python-property-init-v1
  ```
- **Documentation Sync:** All 10 markers and their behavioral impact are documented in `docs/contracts/extraction-output-changes.md` (§3.6.1 and §3.6.2) and `docs/languages/python.md`.

---

### 2.4 Edge Cases and Future Hardening Recommendations

During code review of the Python changes, two edge cases were identified:
1. **String Heuristic in `keep_first_attribute_declaration` (`python/assignments.rs:206-211`):**  
   The check `!signature.contains("def ")` is intended to isolate `@property` methods. If a class variable contains `"def "` in a string literal (e.g. `REGEX = r"^def\s+"`), it is excluded from `declared`.  
   *Hardening:* Check `symbol.body_span.is_some()` or check `symbol.metadata` for property decorators rather than string matching on signatures.
2. **Non-Simple Pattern Bindings in `binds_cls_locally` (`python/helpers.rs:131-152`):**  
   The checker looks for `assignment` where `left.kind() == "identifier"` and `aliased_import`. It does not detect destructuring (`cls, _ = ...`), loop variables (`for cls in ...`), context managers (`with ... as cls`), or walrus operators (`cls := ...`).  
   *Hardening:* Inspect pattern targets across loop, with, and named expression nodes.

---

## 3. Cross-Language Parity Matrix

The matrix below systematically evaluates the 9 construct areas touched by Python's fixes across all 11 major general-purpose, systems, and scripting languages supported in `julie-extractors`:

| # | Construct Area | Python (v3.6.2) | TypeScript | JavaScript | Ruby | Go | Rust | Java | C# | PHP | Kotlin | C++ |
|---|----------------|-----------------|------------|------------|------|----|------|------|----|-----|--------|-----|
| **1** | **1-Based Anonymous Naming** | `lambda_{line}` (1-based, no `<>`) | N/A (callbacks omitted) | N/A (callbacks omitted) | N/A (blocks omitted) | N/A (omitted) | N/A (closures omitted) | N/A (omitted) | `{name}$lambda` | `anon_class_L{line}` (1-based) | N/A (omitted) | **0-based `<anon_...>` (GAP)** |
| **2** | **Instance / Member Docstrings** | Supported (`#:`, docstrings) | **Omitted in ctor (GAP)** | **Trailing omitted (GAP)** | Supported (preceding) | Supported (`//`) | Supported (`///`) | Supported (Javadoc) | Supported (`///`) | Supported | Supported (KDoc) | Supported (`///`, `/**`) |
| **3** | **Property & Attribute Coexistence** | Emits both (no hiding) | **Omitted (GAP)** | **Hidden by getter (GAP)** | Emits both | N/A | N/A | N/A | Distinct symbols | N/A | Supported | N/A |
| **4** | **Decorator / Annotation Args in Sig** | Retains args (max 100 ch) | **Args stripped (GAP)** | N/A | N/A | N/A | Attributes omitted by design | Retains full args in sig | Retains full args in sig | Retains full args in sig | Retains full args in sig | N/A |
| **5** | **No Dangling ` = ` on Bare Declarations** | Clean (no ` = `) | Clean | Clean | Clean | Clean | Clean | Clean | Clean | Clean | Clean | Clean |
| **6** | **Runtime Type Checks as `type_usage`** | `isinstance` -> `type_usage` | **`instanceof` -> `var_ref` (GAP)** | **`instanceof` -> `var_ref` (GAP)** | N/A | Type assertion -> `type_usage` | `match` / `downcast` | `instanceof` -> `type_usage` | `is` -> `type_usage` | `instanceof` -> `type_usage` | `is` / `as` -> `type_usage` | `dynamic_cast` -> `type_usage` |
| **7** | **Parameter-Bound Class Receiver** | `cls` bound via parameter | `this` scoped | `this` scoped | `self` checked | N/A | `Self` scoped | N/A | N/A | `self::` scoped | `this` scoped | N/A |
| **8** | **Grammar Return Syntax & Clean Meta** | `->` in sig, clean meta | `: ` in sig, clean meta | Clean meta | Clean meta | Clean meta | `->` in sig, clean meta | Clean meta | Clean meta | `: ` in sig, clean meta | `: ` in sig, clean meta | Clean meta |
| **9** | **Receiver in Member Signatures** | `self.x = ...` | **Omitted (GAP)** | `this.x = ...` | `@x = ...` | N/A | N/A | N/A | N/A | N/A | N/A | N/A |

---

## 4. Concrete Defects and Parity Gaps Found

### Gap 1: TypeScript Decorator Argument Stripping in Signatures
- **Language:** TypeScript
- **Source File & Line Numbers:** `crates/julie-extractors/src/typescript/helpers.rs:105-135`, `functions.rs:206-218`, `classes.rs:80-92`
- **AST Nodes:** `decorator` -> `call_expression` -> `arguments`
- **Root Cause:** In `typescript/helpers.rs`, `extract_single_decorator_name` only extracts the function identifier from a `call_expression`, explicitly ignoring arguments. When `decorator_prefix` builds the signature prefix, it prepends `@Route ` rather than `@Route('/api/v1/users', { auth: true }) `.
- **Reproduction Test:** `crates/julie-extractors/src/tests/repro_parity_gaps.rs::repro_ts_decorator_arguments_stripped_in_signatures`
- **Impact:** Method and class signatures lose crucial routing, authorization, and configuration context.
- **Required Closure Steps:**
  1. Update `typescript/helpers.rs` to extract full decorator text, collapsing whitespace onto one line and truncating at 100 characters, mirroring `python/decorators.rs`.
  2. Keep bare decorator names in symbol metadata (`metadata["decorators"]`).
  3. Register contract marker `typescript-decorator-signature-arguments-v1`.
  4. Refresh affected TypeScript golden fixtures.

---

### Gap 2: JavaScript & TypeScript `instanceof` Identifier Classification
- **Languages:** JavaScript, TypeScript
- **Source File & Line Numbers:** `crates/julie-extractors/src/javascript/identifiers.rs:175-184, 480-498`
- **AST Nodes:** `binary_expression` with operator `instanceof` -> `right: identifier`
- **Root Cause:** Neither the JS nor TS identifier walker checks for the `instanceof` binary operator. The RHS operand falls through `is_ecmascript_value_read_identifier` to `_ => true`, which creates an identifier with `IdentifierKind::VariableRef`.
- **Reproduction Tests:**  
  - `crates/julie-extractors/src/tests/repro_parity_gaps.rs::repro_js_instanceof_classified_as_variable_ref_instead_of_type_usage`
  - `crates/julie-extractors/src/tests/repro_parity_gaps.rs::repro_ts_instanceof_classified_as_variable_ref_instead_of_type_usage`
- **Impact:** Downstream queries for type references (e.g. "where is `MyClass` used?") fail to match runtime type guards in JavaScript and TypeScript codebases.
- **Required Closure Steps:**
  1. Add an arm to `walk_identifiers` in `javascript/identifiers.rs`: when node is `binary_expression` with operator `instanceof`, emit the right operand as `IdentifierKind::TypeUsage`.
  2. Exclude the RHS of `instanceof` from `is_ecmascript_value_read_identifier`.
  3. Mirror the implementation currently working in PHP (`php/identifiers.rs:198-220`).
  4. Register contract marker `ecmascript-instanceof-type-usage-v1`.

---

### Gap 3: JavaScript Constructor Property Suppression by Getters/Setters
- **Language:** JavaScript
- **Source File & Line Numbers:** `crates/julie-extractors/src/javascript/assignments.rs:177-181`
- **AST Nodes:** `class_body` -> `method_definition` (kind: `get`/`set`) vs. `assignment_expression` (`this.x = value`)
- **Root Cause:**
  ```rust
  if symbols.iter().any(|symbol| {
      symbol.parent_id.as_deref() == Some(&class_symbol.id) && symbol.name == name
  }) {
      return None;
  }
  ```
  If a class defines a getter `get config()`, `extract_constructor_property` detects that a symbol named `"config"` already exists on the class and returns `None`, discarding `this.config = cfg`.
- **Reproduction Test:** `crates/julie-extractors/src/tests/repro_parity_gaps.rs::repro_js_property_suppression_when_getter_exists`
- **Impact:** Indexers cannot see where the backing instance property was initialized.
- **Required Closure Steps:**
  1. Filter out symbols of kind `Method` (or getter/setter symbols) from the deduplication check in `extract_constructor_property`, mirroring Python's fix in `assignments.rs:206-228`.
  2. Register contract marker `javascript-property-init-coexistence-v1`.

---

### Gap 4: C++ Anonymous Union 0-Based and Angle-Bracketed Naming
- **Language:** C++
- **Source File & Line Numbers:** `crates/julie-extractors/src/cpp/types.rs:187`
- **AST Nodes:** `union_specifier` lacking `type_identifier`
- **Root Cause:**
  ```rust
  format!("<anonymous_union_{}>", node.start_position().row)
  ```
  Uses the 0-based AST row index (`node.start_position().row`) and wraps the name in angle brackets `<...>`.
- **Reproduction Tests:**  
  - `crates/julie-extractors/src/tests/repro_parity_gaps.rs::repro_cpp_anonymous_union_zero_based_and_bracketed_naming`
  - `crates/julie-extractors/src/tests/repro_parity_gaps.rs::repro_cpp_anonymous_union_in_struct_zero_based_row_offset`
- **Impact:** Violates product-wide conventions established in Python (`77cd5314`) and PHP: synthetic names must be 1-based to match displayed source lines and must avoid angle brackets that degrade search tokenization.
- **Required Closure Steps:**
  1. Update to `format!("anonymous_union_{}", node.start_position().row + 1)`.
  2. Register contract marker `cpp-anonymous-union-naming-v1`.
  3. Refresh affected C++ golden fixtures.

---

### Gap 5: TypeScript Omission of Constructor-Assigned Instance Properties
- **Language:** TypeScript
- **Source File & Line Numbers:** `crates/julie-extractors/src/typescript/symbols.rs:110-180`
- **AST Nodes:** `method_definition` (constructor) -> `assignment_expression` (`this.x = value`)
- **Root Cause:** While JavaScript extracts `this.x = value` inside constructor bodies as `Property` symbols, TypeScript only extracts explicit class field declarations and parameter properties (`constructor(public x: number)`). Constructor assignments in TypeScript emit no symbols.
- **Reproduction Test:** `crates/julie-extractors/src/tests/repro_parity_gaps.rs::repro_ts_constructor_assigned_properties_omitted_entirely`
- **Impact:** Instance properties assigned in constructors are completely missing from TypeScript symbol tables, and preceding JSDoc comments are lost because there is no symbol to attach them to.
- **Required Closure Steps:**
  1. Port `extract_constructor_property` from JavaScript into the TypeScript symbol extractor.
  2. Ensure parameter properties and class field declarations take precedence over constructor assignments.
  3. Register contract marker `typescript-constructor-assigned-properties-v1`.

---

### Gap 6: JavaScript Trailing Docstrings on Constructor Properties
- **Language:** JavaScript
- **Source File & Line Numbers:** `crates/julie-extractors/src/javascript/assignments.rs:165-203`
- **AST Nodes:** `expression_statement` (`this.x = value;`) followed by `comment` / `expression_statement` string
- **Root Cause:** `find_doc_comment` only searches preceding comments for JavaScript statements. It does not check succeeding sibling statements for trailing docstrings (which Python `77cd5314` supported via PEP 257).
- **Reproduction Test:** `crates/julie-extractors/src/tests/repro_parity_gaps.rs::repro_js_trailing_attribute_docstring_dropped`
- **Impact:** Post-declaration docstrings are dropped.
- **Required Closure Steps:**
  1. In `extract_constructor_property`, inspect succeeding siblings for JSDoc comment blocks or string literal statements if no preceding comment exists.
  2. Register contract marker `javascript-attribute-trailing-doc-v1`.

---

### 4.7 Forensic Audit Note: Verification of Preceding Doc Comments in JS & Ruby
An earlier survey hypothesis asserted that setting `doc_comment: None` in `crates/julie-extractors/src/javascript/assignments.rs:193` and `crates/julie-extractors/src/ruby/assignments.rs:90` caused directly preceding doc comments to be discarded.

**Forensic Investigation:**  
Live test execution (`audit_verification_preceding_doc_comments_rescued_by_base_extractor_fallback`) proved this hypothesis incorrect. In `crates/julie-extractors/src/base/creation_methods.rs:77`, `BaseExtractor::create_symbol_from_span` contains:
```rust
doc_comment: options.doc_comment.or_else(|| self.find_doc_comment(node)),
```
Whenever `options.doc_comment` is `None`, `self.find_doc_comment(node)` executes automatically. In JavaScript, `should_search_ancestor_doc_comments` explicitly permits `expression_statement`, allowing preceding JSDoc blocks to be found. In Ruby, `find_ruby_doc_comment` checks `node.prev_named_sibling()` directly and finds leading `#` comment blocks.

Therefore, directly preceding doc comments on constructor properties in JS and instance variables in Ruby are already functioning correctly. The actual defects in this domain are TypeScript's total omission of constructor-assigned property symbols (Gap 5) and JavaScript's omission of trailing docstrings (Gap 6).

---

## 5. Verification & Quality Gate Results

### 5.1 In-Memory Targeted Reproduction Tests
The new reproduction test suite in `crates/julie-extractors/src/tests/repro_parity_gaps.rs` was executed:
```bash
cargo test --lib repro_parity_gaps
```
**Results:**
```text
running 9 tests
test tests::repro_parity_gaps::repro_js_instanceof_classified_as_variable_ref_instead_of_type_usage ... ok
test tests::repro_parity_gaps::repro_js_property_suppression_when_getter_exists ... ok
test tests::repro_parity_gaps::repro_js_trailing_attribute_docstring_dropped ... ok
test tests::repro_parity_gaps::repro_ts_constructor_assigned_properties_omitted_entirely ... ok
test tests::repro_parity_gaps::repro_ts_instanceof_classified_as_variable_ref_instead_of_type_usage ... ok
test tests::repro_parity_gaps::repro_ts_decorator_arguments_stripped_in_signatures ... ok
test tests::repro_parity_gaps::audit_verification_preceding_doc_comments_rescued_by_base_extractor_fallback ... ok
test tests::repro_parity_gaps::repro_cpp_anonymous_union_zero_based_and_bracketed_naming ... ok
test tests::repro_parity_gaps::repro_cpp_anonymous_union_in_struct_zero_based_row_offset ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 6322 filtered out; finished in 0.01s
```
All 9 tests pass with sub-millisecond execution times.

### 5.2 Full Library Regression Suite
The complete library test suite across all 4 crates was executed:
```bash
cargo test --lib
```
**Results:**
- `julie_extract_artifact`: 5 passed; 0 failed
- `julie_extract_cli`: 156 passed; 0 failed
- `julie_extractors`: 6,324 passed; 0 failed; 7 ignored
- `xtask`: 7 passed; 0 failed
- **Total: 6,492 passed; 0 failed; 7 ignored** (execution time: 6.74s)

### 5.3 Language Data Quality Gate
The strict capability report was executed:
```bash
node scripts/language-data-quality-report.mjs --strict
```
**Results:**
- `silent_cells`: **0**
- `quality_bar_debts`: **0**
- `open_gap_backlog`: 21 (all pre-classified in `structural_facts`)
- `golden fixture registry`: Zero missing or unclassified files.

---

## 6. Actionable Recommendations & Prioritized Implementation Roadmap

To achieve full cross-language parity with the standards established in Python `v3.6.1` and `v3.6.2`, the following prioritized implementation roadmap is recommended for subsequent development milestones:

| Priority | Task ID | Language | Description | Estimated Scope | Contract Marker Required |
|----------|---------|----------|-------------|-----------------|--------------------------|
| **P1** | `TS-DECORATOR-ARGS` | TypeScript | Retain decorator arguments in method and class signatures, formatted on one single line and truncated at 100 characters. | `typescript/helpers.rs`, `functions.rs`, `classes.rs` | `typescript-decorator-signature-arguments-v1` |
| **P1** | `ECMA-INSTANCEOF-TYPE` | JS / TS | Detect `instanceof` binary operator and emit RHS identifier as `IdentifierKind::TypeUsage`. | `javascript/identifiers.rs`, `typescript/identifiers.rs` | `ecmascript-instanceof-type-usage-v1` |
| **P1** | `CPP-UNION-1BASED` | C++ | Transition anonymous union symbol names from `<anonymous_union_{row}>` (0-based) to `anonymous_union_{line}` (1-based, no brackets). | `cpp/types.rs` | `cpp-anonymous-union-naming-v1` |
| **P2** | `JS-PROPERTY-INIT` | JavaScript | Allow constructor-assigned `this.x = value` to emit a `Property` symbol even when a getter or setter exists on the class. | `javascript/assignments.rs` | `javascript-property-init-coexistence-v1` |
| **P2** | `TS-CTOR-PROPERTIES` | TypeScript | Port constructor property extraction (`this.x = value`) from JS to TS, ensuring parameter properties take precedence. | `typescript/symbols.rs` | `typescript-constructor-assigned-properties-v1` |
| **P3** | `PY-HARDENING` | Python | Replace `signature.contains("def ")` check with `body_span.is_some()` or metadata check; expand `binds_cls_locally` to destructuring patterns. | `python/assignments.rs`, `python/helpers.rs` | None (internal hardening) |

---

## 7. Conclusion

The audit confirms that the Python extractor updates across `v3.6.0..HEAD` (`v3.6.1` and `v3.6.2`) are correct, fully tested, and strictly conforming to all architectural contracts. Furthermore, this audit has established exact AST-level specifications and reproducible in-memory test cases for 6 concrete cross-language parity gaps in TypeScript, JavaScript, and C++. The implementation roadmap provides a clear path forward for future releases to achieve uniform extraction quality across all supported language ecosystems.
