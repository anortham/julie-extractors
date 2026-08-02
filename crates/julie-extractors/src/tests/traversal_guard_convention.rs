//! Source-scan convention guard for the crate-wide traversal budget.
//!
//! Every production walker that recurses over CST children must bound itself
//! with `tree_traversal::should_visit_tree_depth` / `child_tree_depth`. An
//! unguarded walker costs one Rust frame per CST node, and a generated source
//! file with a 17k-deep expression spine then overflows the extraction worker's
//! stack — which aborts the process instead of failing the file, so no
//! per-file recovery path can catch it.
//!
//! Mutual recursion (A -> B -> A) is covered as well as direct self-recursion:
//! a grep for a function calling itself misses the cycle that shipped the crash.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

const CHILD_ITERATION_MARKER: &str = "children(&mut";
const GUARD_MARKERS: [&str; 3] = [
    "should_visit_tree_depth",
    "should_visit_bounded_depth",
    "child_tree_depth",
];

struct WalkerFn {
    name: String,
    file: PathBuf,
    line: usize,
    iterates_children: bool,
    references_guard: bool,
    calls: Vec<String>,
}

#[test]
fn production_tree_walkers_bound_themselves_with_the_traversal_budget() {
    let functions = production_functions();
    assert!(
        functions.len() > 500,
        "source scan should see the whole production crate, found only {} functions",
        functions.len()
    );
    assert!(
        functions.iter().any(|function| function.iterates_children),
        "source scan should find CST child iteration; the marker probably drifted"
    );

    let graph = CallGraph::build(&functions);
    let mut violations = Vec::new();

    for (index, function) in functions.iter().enumerate() {
        if !function.iterates_children {
            continue;
        }
        let cycle = graph.recursion_cycle(index);
        if cycle.is_empty() {
            continue;
        }
        if cycle
            .iter()
            .any(|member| functions[*member].references_guard)
        {
            continue;
        }
        violations.push(describe(&functions, index, &cycle));
    }

    assert!(
        violations.is_empty(),
        "unguarded recursive tree walkers found — each must call \
         `should_visit_tree_depth` before visiting and `child_tree_depth` before \
         descending:\n{}",
        violations.join("\n")
    );
}

fn describe(functions: &[WalkerFn], index: usize, cycle: &BTreeSet<usize>) -> String {
    let function = &functions[index];
    let members: Vec<&str> = cycle
        .iter()
        .filter(|member| **member != index)
        .map(|member| functions[*member].name.as_str())
        .collect();
    let via = if members.is_empty() {
        "self-recursion".to_string()
    } else {
        format!("recursion cycle with {}", members.join(", "))
    };
    format!(
        "  {}:{} {} ({via})",
        function.file.display(),
        function.line,
        function.name
    )
}

struct CallGraph {
    edges: Vec<BTreeSet<usize>>,
    reverse_edges: Vec<BTreeSet<usize>>,
}

impl CallGraph {
    fn build(functions: &[WalkerFn]) -> Self {
        let mut by_name: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (index, function) in functions.iter().enumerate() {
            by_name
                .entry(function.name.as_str())
                .or_default()
                .push(index);
        }

        let mut edges = vec![BTreeSet::new(); functions.len()];
        let mut reverse_edges = vec![BTreeSet::new(); functions.len()];
        for (index, function) in functions.iter().enumerate() {
            for call in &function.calls {
                let Some(candidates) = by_name.get(call.as_str()) else {
                    continue;
                };
                let same_file: Vec<usize> = candidates
                    .iter()
                    .copied()
                    .filter(|candidate| functions[*candidate].file == function.file)
                    .collect();
                // A crate-wide name map cannot tell two same-named private
                // helpers apart, so resolve within the file first and otherwise
                // only when the name is unique crate-wide. Ambiguous names are
                // dropped rather than fanned out into invented cycles.
                let resolved = if !same_file.is_empty() {
                    same_file
                } else if candidates.len() == 1 {
                    candidates.clone()
                } else {
                    Vec::new()
                };
                for target in resolved {
                    edges[index].insert(target);
                    reverse_edges[target].insert(index);
                }
            }
        }

        Self {
            edges,
            reverse_edges,
        }
    }

    fn recursion_cycle(&self, start: usize) -> BTreeSet<usize> {
        let forward = reachable(&self.edges, start);
        if !forward.contains(&start) {
            return BTreeSet::new();
        }
        let backward = reachable(&self.reverse_edges, start);
        forward
            .intersection(&backward)
            .copied()
            .collect::<BTreeSet<usize>>()
    }
}

fn reachable(edges: &[BTreeSet<usize>], start: usize) -> HashSet<usize> {
    let mut seen = HashSet::new();
    let mut stack: Vec<usize> = edges[start].iter().copied().collect();
    while let Some(node) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        stack.extend(edges[node].iter().copied());
    }
    seen
}

fn production_functions() -> Vec<WalkerFn> {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let language = crate::language::get_tree_sitter_language("rust")
        .expect("the crate extracts rust, so its own grammar must load");
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("rust grammar should configure a parser");

    let mut functions = Vec::new();
    for path in production_sources(&source_root) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let tree = parser
            .parse(&source, None)
            .unwrap_or_else(|| panic!("failed to parse {}", path.display()));
        collect_functions(tree.root_node(), &source, &path, &mut functions);
    }
    functions
}

fn production_sources(root: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", directory.display()));
        for entry in entries {
            let path = entry
                .expect("source directory entry should be readable")
                .path();
            if path.is_dir() {
                if path.file_name().and_then(|name| name.to_str()) != Some("tests") {
                    stack.push(path);
                }
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    sources
}

fn collect_functions(node: Node<'_>, source: &str, path: &Path, functions: &mut Vec<WalkerFn>) {
    if node.kind() == "function_item"
        && let Some(function) = walker_fn(node, source, path)
    {
        functions.push(function);
    }
    if node.kind() == "mod_item" && is_test_module(node, source) {
        return;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_functions(child, source, path, functions);
    }
}

fn is_test_module(node: Node<'_>, source: &str) -> bool {
    node.child_by_field_name("name")
        .and_then(|name| node_text(source, name))
        .is_some_and(|name| name.contains("test"))
}

fn walker_fn(node: Node<'_>, source: &str, path: &Path) -> Option<WalkerFn> {
    let name = node_text(source, node.child_by_field_name("name")?)?.to_string();
    let body = node.child_by_field_name("body")?;
    let body_text = node_text(source, body)?;

    let mut calls = Vec::new();
    collect_calls(body, source, &mut calls);

    Some(WalkerFn {
        name,
        file: path.to_path_buf(),
        line: node.start_position().row + 1,
        iterates_children: body_text.contains(CHILD_ITERATION_MARKER),
        references_guard: GUARD_MARKERS
            .iter()
            .any(|marker| body_text.contains(marker)),
        calls,
    })
}

fn collect_calls(node: Node<'_>, source: &str, calls: &mut Vec<String>) {
    if node.kind() == "call_expression"
        && let Some(function) = node.child_by_field_name("function")
        && let Some(name) = called_name(function, source)
    {
        calls.push(name.to_string());
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_calls(child, source, calls);
    }
}

fn called_name<'a>(function: Node<'_>, source: &'a str) -> Option<&'a str> {
    match function.kind() {
        "identifier" => node_text(source, function),
        "scoped_identifier" => function
            .child_by_field_name("name")
            .and_then(|name| node_text(source, name)),
        "generic_function" => function
            .child_by_field_name("function")
            .and_then(|inner| called_name(inner, source)),
        _ => None,
    }
}

fn node_text<'a>(source: &'a str, node: Node<'_>) -> Option<&'a str> {
    source.get(node.start_byte()..node.end_byte())
}
