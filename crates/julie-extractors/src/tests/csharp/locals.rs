use crate::base::SymbolKind;
use crate::csharp::CSharpExtractor;
use std::path::PathBuf;

fn extract(source: &str) -> (Vec<crate::base::Symbol>, CSharpExtractor) {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = CSharpExtractor::new(
        "csharp".to_string(),
        "locals.cs".to_string(),
        source.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

#[test]
fn emits_local_variables_and_parameters_as_symbols() {
    let source = r#"
namespace App {
  public class Service {
    public int Run(int seed, string label) {
      int total = seed;
      var name = label;
      return total;
    }
  }
}
"#;
    let (symbols, _) = extract(source);
    let by_name = |n: &str| {
        symbols
            .iter()
            .find(|s| s.name == n)
            .unwrap_or_else(|| panic!("missing symbol {n}"))
    };

    let seed = by_name("seed");
    assert_eq!(seed.kind, SymbolKind::Variable);
    assert_eq!(
        seed.metadata
            .as_ref()
            .and_then(|m| m.get("role"))
            .and_then(|v| v.as_str()),
        Some("parameter")
    );
    assert_eq!(
        seed.metadata
            .as_ref()
            .and_then(|m| m.get("variableType"))
            .and_then(|v| v.as_str()),
        Some("int")
    );

    let total = by_name("total");
    assert_eq!(total.kind, SymbolKind::Variable);
    assert_eq!(
        total
            .metadata
            .as_ref()
            .and_then(|m| m.get("role"))
            .and_then(|v| v.as_str()),
        Some("local")
    );
    assert_eq!(
        total
            .metadata
            .as_ref()
            .and_then(|m| m.get("variableType"))
            .and_then(|v| v.as_str()),
        Some("int")
    );

    let name = by_name("name");
    assert_eq!(
        name.metadata
            .as_ref()
            .and_then(|m| m.get("variableType"))
            .and_then(|v| v.as_str()),
        Some("var")
    );

    let run = by_name("Run");
    assert_eq!(seed.parent_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(total.parent_id.as_deref(), Some(run.id.as_str()));
}

#[test]
fn infers_declared_types_for_locals_and_parameters() {
    let source = r#"
namespace App {
  public class Widget {
    public void Use(Fixture fixture) {
      string label = "x";
      var unknown = label;
    }
  }
  public class Fixture {
    public int Value;
  }
}
"#;
    let (symbols, extractor) = extract(source);
    let types = extractor.infer_types(&symbols);
    let id = |n: &str| {
        symbols
            .iter()
            .find(|s| s.name == n)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| panic!("missing {n}"))
    };

    assert_eq!(
        types.get(&id("fixture")).map(String::as_str),
        Some("Fixture")
    );
    assert_eq!(types.get(&id("label")).map(String::as_str), Some("string"));
}

#[test]
fn out_ref_and_foreach_emit_locals_but_multiplication_does_not() {
    let source = r#"
namespace App {
  public class Service {
    private const int Scale = 4;
    public int Scaled(int requested) {
      return requested * Scale;
    }
    public void TryParse(string input) {
      if (int.TryParse(input, out int value)) {
        _ = value;
      }
      if (int.TryParse(input, out var inferred)) {
        _ = inferred;
      }
      foreach (string item in new[] { "a" }) {
        _ = item;
      }
    }
  }
}
"#;
    let (symbols, _) = extract(source);
    let locals: Vec<_> = symbols
        .iter()
        .filter(|s| {
            s.kind == SymbolKind::Variable
                && s.metadata
                    .as_ref()
                    .and_then(|m| m.get("role"))
                    .and_then(|v| v.as_str())
                    == Some("local")
        })
        .map(|s| s.name.as_str())
        .collect();

    assert!(
        !locals.contains(&"Scale"),
        "multiplication operand must not become a local: {locals:?}"
    );
    assert!(
        locals.contains(&"value"),
        "out int value should emit a local: {locals:?}"
    );
    assert!(
        locals.contains(&"inferred"),
        "out var inferred should emit a local: {locals:?}"
    );
    assert!(
        locals.contains(&"item"),
        "foreach loop variable should emit a local: {locals:?}"
    );

    let value = symbols.iter().find(|s| s.name == "value").unwrap();
    assert_eq!(
        value
            .metadata
            .as_ref()
            .and_then(|m| m.get("variableType"))
            .and_then(|v| v.as_str()),
        Some("int")
    );
}
