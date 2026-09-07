# Contract: Rust Host Syntax API

Status: Active Contract  
Feature Gate: `syntax-api` (optional, off by default)  
Release: Unreleased  
Upstream Crate: `julie-extractors`  

## 1. Scope & Product Boundary

The `syntax-api` feature exposes a zero-copy, path-aware host syntax parsing interface and typed relationship fact exports for downstream Rust callers (such as Julie):
- **Upstream responsibility:** Parse raw source strings using path-aware grammar selection, reuse pre-parsed probe trees (C/C++ `.h`), collect iterative parse diagnostics without stack exhaustion, and enforce cooperative cancellation/deadlines.
- **Downstream responsibility:** Downstream consumers own semantic interpretation, edit validation, before/after error comparisons, symbol rewrites, and worker thread pool concurrency bounds.
- **Encapsulation:** Internal modules (`base`, `pipeline`, `language_spec`, `syntax::diagnostics`, `syntax::source_detection`) remain strictly private. No legacy `ExtractorManager`, `Symbol.code_context`, or public `get_tree_sitter_language` helper is restored.

## 2. Public API Surface

### 2.1 Crate Root Exports
```rust
#[cfg(feature = "syntax-api")]
pub mod syntax;

pub use base::relationship_resolution::{PendingSpan, UnresolvedTarget};
```

### 2.2 Syntax Module (`julie_extractors::syntax`)
```rust
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use crate::ParseDiagnostic;

#[derive(Debug)]
pub struct ParsedSource {
    pub language: &'static str,
    pub tree: tree_sitter::Tree,
    pub diagnostics: Vec<ParseDiagnostic>,
}

#[derive(Debug)]
pub enum SyntaxError {
    UnsupportedLanguage { path: PathBuf },
    UnsupportedContainer { path: PathBuf },
    InputTooLarge { bytes: usize },
    Cancelled,
    DeadlineExceeded,
    ParseFailed { source: anyhow::Error },
}

pub struct SyntaxOptions<'a> {
    pub deadline: Option<Instant>,
    pub cancelled: Option<&'a AtomicBool>,
    pub max_source_bytes: usize,
}

pub fn parse_source(file_path: &Path, source: &str) -> Result<ParsedSource, SyntaxError>;

pub fn parse_source_with_options(
    file_path: &Path,
    source: &str,
    options: &SyntaxOptions<'_>,
) -> Result<ParsedSource, SyntaxError>;
```

**Source Ownership and Lifetime:**
`ParsedSource` owns its tree and diagnostics, but owns no source text. The caller must retain the exact original UTF-8 source string when slicing node byte ranges or diagnostic spans. No public API returns a `Node` detached from its tree.

### 2.3 `SyntaxOptions` Defaults
- `deadline: None` (no timeout)
- `cancelled: None` (no external cancellation flag)
- `max_source_bytes: (u32::MAX - 1) as usize` (4,294,967,294 bytes)

## 3. Coordinate Systems & Span Invariants

All spans and diagnostics reference the exact, unmutated UTF-8 source string provided by the caller:

| Field | Indexing | Unit | Invariant / Boundary | Description |
|---|---|---|---|---|
| `start_byte` / `end_byte` | 0-based | UTF-8 bytes | `0 <= start_byte <= end_byte <= source.len()` | Byte offsets into the caller's UTF-8 string |
| `start_line` / `end_line` | 1-based | Lines | `1 <= start_line <= end_line` | Newline character count (`\n`) + 1 |
| `start_column` / `end_column` | 0-based | UTF-8 bytes | `0 <= column <= line_byte_length` | Byte offset from the start of the current line |
| `Point.row` | 0-based | Rows | `0 <= row < total_lines` | Tree-sitter native point row |
| `Point.column` | 0-based | UTF-8 bytes | `0 <= column <= line_byte_length` | Tree-sitter native point column |

*Note: Columns are byte columns, not character counts or UTF-16 code units.*

## 4. Error Table

| Variant | Trigger Condition | Recoverable? | Details |
|---|---|---|---|
| `UnsupportedLanguage { path }` | Extension or file name unrecognized among all 40 supported languages | No | Returns unmutated caller path |
| `UnsupportedContainer { path }` | File path has `.jsonl` extension or name (case-insensitive) | No | Line-delimited containers refused by single-tree API |
| `InputTooLarge { bytes }` | `bytes > max_source_bytes` or `bytes > u32::MAX - 1` | No | Checked before allocation/parsing to prevent index overflow |
| `Cancelled` | `options.cancelled` atomic flag is `true` | Yes | Takes precedence over deadline when both trigger |
| `DeadlineExceeded` | `Instant::now() >= deadline` | Yes | Checked during language detection, parser progress, and traversal |
| `ParseFailed { source }` | Tree-sitter initialization error or parser returned `None` tree | No | Wraps underlying `anyhow::Error` via `source()` |

## 5. Cancellation, Deadlines, & Concurrency Limits

1. **Cooperative Cancellation Mechanism:**
   Cancellation and deadlines are checked:
   - Before parser invocation.
   - Inside Tree-sitter's `progress_callback` during parsing (`ControlFlow::Break(())`).
   - Between probe parses for C/C++ headers.
   - At each iteration of the diagnostic AST cursor traversal.
2. **Scanner Stalls & External Batching:**
   - Tree-sitter 0.26.11 batches initial state transitions; sources under ~1.5KB may complete before triggering `progress_callback`.
   - Native C tree-sitter external scanners (e.g. indentation scanners in Python/YAML) execute synchronously; cooperative cancellation takes effect once control returns to Tree-sitter.
3. **Downstream Thread Pool Bounds & Worker Lifetime:**
   - Dropping an async task or future in downstream runtimes (e.g. Tokio) does not terminate a CPU thread running in native C code. Downstream callers must use bounded worker thread pools with timeouts.
   - **Permit Retention:** Callers must retain each job's admission permit until the blocking worker actually returns and joins; releasing a permit merely because an awaiting request timed out can defeat the concurrency bound and exhaust worker threads.

## 6. Grammar Selection & Heuristics

- **C/C++ Headers (`.h`, `.H`):** Disambiguation executes at most two probes (`parser_c` and `parser_cpp`). The winning parse tree is reused directly for `ParsedSource.tree`, incurring **zero third parse**. Probe errors propagate immediately as `SyntaxError` without falling back to C.
- **F# Signature Files (`.fsi`):** Dispatches to `tree_sitter_fsharp::LANGUAGE_SIGNATURE`. Other F# extensions (`.fs`, `.fsx`) dispatch to standard `LANGUAGE`.
- **Extensionless Files:** Exact base names (e.g. `qmldir`) are matched case-insensitively.
- **Case Sensitivity:** File extensions are evaluated case-insensitively (e.g. `.RS`, `.JSONL`, `.H`).

## 7. Container & Composition Limits

- **Host Tree Coverage:** The returned `tree` covers the entire source string (`0..source.len()`).
- **Composite Files (Vue, HTML, Razor, Markdown):** Returns the host template/markup tree only. Embedded language islands (e.g. `<script>` JavaScript inside Vue or HTML) are not parsed as child trees. Downstream consumers needing embedded extractions must use `extract_canonical`.
- **JSONL Refusal:** Case-insensitive `.jsonl` files are rejected with `SyntaxError::UnsupportedContainer`. Full JSONL extraction remains available in `extract_canonical`.

## 8. Capability Fixture Ledger (40 Languages)

Every capability language entry in `fixtures/extraction/capabilities.json` is backed by a non-JSONL host syntax fixture:

| Language | Selected Host Fixture Path |
|---|---|
| `bash` | `fixtures/extraction/bash/basic/source.sh` |
| `c` | `fixtures/extraction/c/basic/source.c` |
| `cpp` | `fixtures/extraction/cpp/basic/source.cpp` |
| `csharp` | `fixtures/extraction/csharp/basic/source.cs` |
| `css` | `fixtures/extraction/css/basic/source.css` |
| `dart` | `fixtures/extraction/dart/basic/source.dart` |
| `elixir` | `fixtures/extraction/elixir/basic/source.ex` |
| `erlang` | `fixtures/extraction/erlang/basic/source.erl` |
| `fsharp` | `fixtures/extraction/fsharp/basic/source.fs` |
| `gdscript` | `fixtures/extraction/gdscript/basic/source.gd` |
| `go` | `fixtures/extraction/go/basic/source.go` |
| `html` | `fixtures/extraction/html/basic/source.html` |
| `java` | `fixtures/extraction/java/basic/source.java` |
| `javascript` | `fixtures/extraction/javascript/basic/source.js` |
| `json` | `fixtures/extraction/json/basic/source.json` |
| `jsx` | `fixtures/extraction/jsx/basic/source.jsx` |
| `kotlin` | `fixtures/extraction/kotlin/basic/source.kt` |
| `lua` | `fixtures/extraction/lua/basic/source.lua` |
| `markdown` | `fixtures/extraction/markdown/basic/source.md` |
| `php` | `fixtures/extraction/php/basic/source.php` |
| `powershell` | `fixtures/extraction/powershell/basic/source.ps1` |
| `python` | `fixtures/extraction/python/basic/source.py` |
| `qml` | `fixtures/extraction/qml/basic/source.qml` |
| `qmldir` | `fixtures/extraction/qmldir/basic/qmldir` |
| `r` | `fixtures/extraction/r/basic/source.r` |
| `razor` | `fixtures/extraction/razor/basic/source.razor` |
| `regex` | `fixtures/extraction/regex/basic/source.regex` |
| `ruby` | `fixtures/extraction/ruby/basic/source.rb` |
| `rust` | `fixtures/extraction/rust/basic/source.rs` |
| `scala` | `fixtures/extraction/scala/basic/source.scala` |
| `sql` | `fixtures/extraction/sql/basic/source.sql` |
| `swift` | `fixtures/extraction/swift/basic/source.swift` |
| `toml` | `fixtures/extraction/toml/basic/source.toml` |
| `tsx` | `fixtures/extraction/tsx/basic/source.tsx` |
| `typescript` | `fixtures/extraction/typescript/basic/source.ts` |
| `vbnet` | `fixtures/extraction/vbnet/basic/source.vb` |
| `vue` | `fixtures/extraction/vue/basic/source.vue` |
| `xml` | `fixtures/extraction/xml/basic/source.xml` |
| `yaml` | `fixtures/extraction/yaml/basic/source.yaml` |
| `zig` | `fixtures/extraction/zig/basic/source.zig` |

Language capabilities can be queried dynamically via `julie_extractors::capability_snapshot().languages()`.

## 9. Diagnostic Example: Recovered Tree is Not Edit Approval

```rust
use std::path::Path;
use julie_extractors::syntax::{parse_source, SyntaxError};

let broken_source = "fn calc( { return 42; }";
let parsed = parse_source(Path::new("calc.rs"), broken_source)?;

// Tree-sitter recovers an AST despite syntax errors:
assert!(parsed.tree.root_node().has_error());

// Parse diagnostics report exact error/missing spans:
for d in &parsed.diagnostics {
    eprintln!("Diagnostic at {}:{}: {:?}", d.start_line, d.start_column, d.kind);
}

// DOWNSTREAM CALLER GUIDANCE:
// A successful `ParsedSource` with non-empty diagnostics means Tree-sitter produced
// an error-recovered tree. It is NOT approval to apply an edit or assume valid code.
// The consumer (e.g. Julie) must decide whether to reject operations on trees with
// pre-existing errors.
```
