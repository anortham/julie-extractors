use std::path::Path;

use crate::ExtractionResults;
use crate::base::{
    ParseDiagnosticKind, collect_complexity_metrics, collect_framework_structural_facts,
};
use crate::pipeline::{extract_canonical, parse_for_language};
use crate::tree_traversal::TREE_TRAVERSAL_DEPTH_LIMIT;

/// Sixteen times the traversal budget, mirroring dotnet/runtime's generated
/// `src/tests/JIT/Regression/JitBlue/GitHub_10215.cs` — one statement chaining
/// 17,602 `+` operators, which tree-sitter parses as an equally deep spine.
const DEEP_CHAIN_TERMS: usize = 16 * TREE_TRAVERSAL_DEPTH_LIMIT as usize;

/// Just past the budget: enough to trip truncation without paying for a
/// whole-pipeline debug extraction of a 16k-node tree.
const OVER_BUDGET_NESTING: usize = TREE_TRAVERSAL_DEPTH_LIMIT as usize + 16;

/// Sized for the traversal budget, never for the tree: a guarded walker costs
/// at most `TREE_TRAVERSAL_DEPTH_LIMIT` frames, so this holds no matter how deep
/// the source goes, while an unguarded walk of `DEEP_CHAIN_TERMS` nodes needs 16
/// times as much and aborts the process.
///
/// Measured against the fattest walker on this path, `CSharpExtractor::walk_tree`,
/// whose *debug* frames run 4-8 KiB — 1,024 of them exceed 4 MiB. The release
/// build fits the same recursion in the 2 MiB the scan pool gives a worker.
const WORKER_STACK_BYTES: usize = 8 * 1024 * 1024;

fn on_stack<T: Send + 'static>(stack_bytes: usize, body: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .name("deep-tree-traversal-budget".to_string())
        .stack_size(stack_bytes)
        .spawn(body)
        .expect("traversal budget test thread should spawn")
        .join()
        .expect("traversal budget test thread should pass")
}

fn csharp_expression_chain(terms: usize) -> String {
    let mut source = String::from("class Deep\n{\n    int Sum(int b)\n    {\n        return b");
    for _ in 0..terms {
        source.push_str(" + b");
    }
    source.push_str(";\n    }\n}\n");
    source
}

fn csharp_nested_blocks(levels: usize) -> String {
    let mut source = String::from("class Deep\n{\n    int Sum(int b)\n    {\n");
    for _ in 0..levels {
        source.push_str("{\n");
    }
    source.push_str("int tooDeep = b;\n");
    for _ in 0..levels {
        source.push_str("}\n");
    }
    source.push_str("        return b;\n    }\n}\n");
    source
}

fn razor_element_nest(terms: usize) -> String {
    let mut source = String::from("<a href=\"/dashboard\">top</a>\n");
    for _ in 0..terms {
        source.push_str("<div>");
    }
    source.push_str("<a href=\"/too-deep\">deep</a>");
    for _ in 0..terms {
        source.push_str("</div>");
    }
    source.push('\n');
    source
}

fn depth_truncation_diagnostics(results: &ExtractionResults) -> usize {
    results
        .parse_diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == ParseDiagnosticKind::DepthTruncated)
        .count()
}

fn route_targets(results: &ExtractionResults) -> Vec<&str> {
    results
        .structural_facts
        .iter()
        .filter_map(|fact| fact.metadata.as_ref()?.get("target_path")?.as_str())
        .collect()
}

#[test]
fn framework_and_complexity_walkers_survive_a_deep_chain_on_a_small_stack() {
    on_stack(WORKER_STACK_BYTES, || {
        let source = csharp_expression_chain(DEEP_CHAIN_TERMS);
        let tree = parse_for_language("csharp", "src/Deep.cs", &source)
            .expect("deep C# should parse")
            .expect("tree-sitter parses deep expression spines iteratively");

        let facts =
            collect_framework_structural_facts("csharp", &tree, "src/Deep.cs", &source, &[]);
        let metrics = collect_complexity_metrics("csharp", &tree, &source, "src/Deep.cs", &[]);

        assert!(
            facts.is_empty(),
            "the chain declares no routes; the walkers must simply return"
        );
        assert!(
            metrics.iter().any(|metric| metric.scope == "file"),
            "complexity metrics must still be produced for an over-budget tree"
        );
        assert!(
            metrics
                .iter()
                .all(|metric| metric.max_nesting_depth <= TREE_TRAVERSAL_DEPTH_LIMIT),
            "nesting depth cannot exceed the traversal budget once collect_stats is guarded"
        );
    });
}

#[test]
fn razor_element_walkers_survive_a_deep_nest_on_a_small_stack() {
    on_stack(WORKER_STACK_BYTES, || {
        let source = razor_element_nest(DEEP_CHAIN_TERMS);
        let tree = parse_for_language("razor", "src/Deep.razor", &source)
            .expect("deep razor should parse")
            .expect("tree-sitter parses deep element nests iteratively");

        let facts =
            collect_framework_structural_facts("razor", &tree, "src/Deep.razor", &source, &[]);

        assert!(
            !facts.iter().any(|fact| {
                fact.metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("target_path"))
                    .and_then(|target| target.as_str())
                    == Some("/too-deep")
            }),
            "href facts below the traversal budget must be dropped, not recursed into"
        );
    });
}

#[test]
fn csharp_extraction_over_budget_records_one_depth_truncation_diagnostic() {
    let results = on_stack(WORKER_STACK_BYTES, || {
        let source = csharp_nested_blocks(OVER_BUDGET_NESTING);
        extract_canonical("src/Deep.cs", &source, Path::new("/repo"))
            .expect("an over-budget tree extracts with capped facts, it does not fail")
    });

    assert!(
        results.symbols.iter().any(|symbol| symbol.name == "Sum"),
        "symbols above the budget must still be extracted"
    );
    assert_eq!(
        depth_truncation_diagnostics(&results),
        1,
        "depth-capped extraction must be reported once, not silently truncated"
    );
    let message = results
        .parse_diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == ParseDiagnosticKind::DepthTruncated)
        .and_then(|diagnostic| diagnostic.message.as_deref())
        .expect("the diagnostic must explain what was dropped");
    assert!(
        message.contains(&TREE_TRAVERSAL_DEPTH_LIMIT.to_string()),
        "diagnostic message must name the budget it hit; got `{message}`"
    );
}

#[test]
fn razor_extraction_over_budget_caps_facts_and_reports_truncation() {
    let results = on_stack(WORKER_STACK_BYTES, || {
        let source = razor_element_nest(OVER_BUDGET_NESTING);
        extract_canonical("src/Deep.razor", &source, Path::new("/repo"))
            .expect("an over-budget razor tree extracts with capped facts")
    });

    assert_eq!(
        depth_truncation_diagnostics(&results),
        1,
        "razor extraction must report the same truncation as every other language"
    );
    let targets = route_targets(&results);
    assert!(
        targets.contains(&"/dashboard"),
        "routes above the budget must still be extracted; got {targets:?}"
    );
    assert!(
        !targets.contains(&"/too-deep"),
        "routes below the budget must be dropped; got {targets:?}"
    );
}

#[test]
fn extraction_within_budget_records_no_depth_truncation_diagnostic() {
    let results = extract_canonical(
        "src/Shallow.cs",
        "class Shallow\n{\n    int Sum(int b) => b + b;\n}\n",
        Path::new("/repo"),
    )
    .expect("shallow source should extract");

    assert_eq!(
        depth_truncation_diagnostics(&results),
        0,
        "a tree within the traversal budget must not claim truncation"
    );
}
