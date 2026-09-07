# julie-extractors

Tree-sitter-based extraction for `julie-extract` artifacts and Rust callers.

## Public Surface

### Canonical Extraction
Use canonical extraction entrypoints:

- `extract_canonical(file_path, content, workspace_root)`
- `extract_canonical_at(file_path, content, workspace_root, level)`

The old pre-parsed-tree factory helper is internal-only. External callers should not bypass the canonical pipeline.

### Relationship Fact Exports
Downstream crates building relationship resolution pipelines can name the structured relationship fact types directly from the crate root:

- `julie_extractors::PendingSpan`
- `julie_extractors::UnresolvedTarget`

### Optional Host Syntax API (`syntax-api` feature)
For consumers (such as Julie) requiring high-fidelity Tree-sitter AST access and iterative parse diagnostics without extracting canonical database rows:

```toml
[dependencies]
julie-extractors = { version = "2.40.6", default-features = false, features = ["syntax-api"] }
```

Entrypoints in module `julie_extractors::syntax`:
- `parse_source(file_path, source)`
- `parse_source_with_options(file_path, source, options)`

Returns `ParsedSource { language, tree, diagnostics }` or typed `SyntaxError`. See [Rust Syntax API Contract](../../docs/contracts/rust-syntax-api.md) for full specification, coordinate tables, error variants, and cancellation semantics.

## Result Semantics

`ExtractionResults` contains:

- `symbols`
- `identifiers`
- `relationships`
- `pending_relationships`
- `structured_pending_relationships`
- `types`

### Paths and IDs

- Stored file paths are normalized relative Unix-style paths.
- Symbol and identifier IDs are derived from normalized path plus normalized location.
- JSONL records are rekeyed after record offsets are applied, so repeated keys on different lines keep distinct IDs.

### JSONL

- `.jsonl` files go through the canonical production path.
- Records are parsed line-by-line as JSON.
- Returned spans use file-global line and byte positions.
- Empty lines are skipped without collapsing later record positions.

### Unresolved Relationships

- `structured_pending_relationships` is the canonical unresolved-edge surface.
- Each entry preserves a structured `target` with terminal name plus any receiver, namespace path, or import context the extractor can prove.
- `pending_relationships` remains as degraded compatibility output for consumers that still read the legacy shape.
- Wrong edges are treated as worse than missing edges, so ambiguous cross-file targets stay pending instead of being force-resolved by name.

## Supported Languages

The crate ships 34 concrete extractors:

- Systems: Rust, C, C++, Go, Zig
- Web: TypeScript, JavaScript, HTML, CSS, Vue, QML
- Backend: Python, Java, C#, PHP, Ruby, Swift, Kotlin, Dart
- Functional: Elixir, F#, Scala
- Scripting: Lua, R, Bash, PowerShell
- Specialized: GDScript, Razor, SQL, Regex
- Documentation and data: Markdown, JSON, TOML, YAML

The registry also exposes JSX and TSX aliases on top of the JavaScript and TypeScript extractors.

## Minimal Example

```rust,no_run
use julie_extractors::{extract_canonical, extract_canonical_at, ExtractionLevel};
use std::path::Path;

let workspace_root = Path::new("/workspace/project");
let file_path = "src/main.ts";
let content = "export function greet() { return 'hi' }";

let canonical = extract_canonical(file_path, content, workspace_root)?;
let canonical_at = extract_canonical_at(file_path, content, workspace_root, ExtractionLevel::Full)?;

assert_eq!(canonical_at.symbols, canonical.symbols);
# Ok::<(), anyhow::Error>(())
```

## Syntax API Example

```rust,no_run
use julie_extractors::syntax::{parse_source, SyntaxError};
use std::path::Path;

let source = "fn compute() { let x = 42; }";
let parsed = parse_source(Path::new("src/lib.rs"), source)?;

assert!(parsed.diagnostics.is_empty());
let root = parsed.tree.root_node();
println!("Parsed {} root node: {}", parsed.language, root.kind());
# Ok::<(), SyntaxError>(())
```
