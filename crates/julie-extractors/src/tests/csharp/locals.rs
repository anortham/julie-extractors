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
fn parameters_of_constructors_local_functions_indexers_and_operators_link_to_their_callable() {
    let source = r#"
namespace App {
  public class Service {
    public Service(int seed) { }
    public int this[int index] => index;
    public static Service operator +(Service left, Service right) => left;
    public int Run(int outer) {
      int Local(int inner) => inner + outer;
      return Local(outer);
    }
  }
}
"#;
    let (symbols, _) = extract(source);
    let symbol = |name: &str| {
        symbols
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("missing symbol {name}"))
    };
    let parameter = |name: &str| {
        let found = symbol(name);
        assert_eq!(found.kind, SymbolKind::Variable);
        assert_eq!(
            found
                .metadata
                .as_ref()
                .and_then(|m| m.get("role"))
                .and_then(|v| v.as_str()),
            Some("parameter"),
            "{name} must carry role=parameter"
        );
        found
    };

    let constructor = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Constructor)
        .expect("missing constructor symbol");
    assert_eq!(
        parameter("seed").parent_id.as_deref(),
        Some(constructor.id.as_str())
    );
    assert_eq!(
        parameter("index").parent_id.as_deref(),
        Some(symbol("this[int index]").id.as_str())
    );
    let operator = symbol("operator +");
    assert_eq!(
        parameter("left").parent_id.as_deref(),
        Some(operator.id.as_str())
    );
    assert_eq!(
        parameter("right").parent_id.as_deref(),
        Some(operator.id.as_str())
    );
    assert_eq!(
        parameter("inner").parent_id.as_deref(),
        Some(symbol("Local").id.as_str())
    );
    assert_eq!(
        parameter("outer").parent_id.as_deref(),
        Some(symbol("Run").id.as_str())
    );
}

#[test]
fn lambda_parameters_link_to_the_lambda_symbol() {
    let source = r#"
namespace App {
  public class Service {
    public void Run() {
      System.Func<int, int> typed = (int explicitParam) => explicitParam;
      System.Func<int, int> bare = simpleParam => simpleParam;
    }
  }
}
"#;
    let (symbols, _) = extract(source);
    let parameters: Vec<_> = symbols
        .iter()
        .filter(|s| {
            s.metadata
                .as_ref()
                .and_then(|m| m.get("role"))
                .and_then(|v| v.as_str())
                == Some("parameter")
        })
        .collect();

    let explicit = parameters
        .iter()
        .find(|s| s.name == "explicitParam")
        .expect("typed lambda parameter must be a symbol");
    let explicit_parent = symbols
        .iter()
        .find(|s| Some(s.id.as_str()) == explicit.parent_id.as_deref())
        .expect("typed lambda parameter must have a parent");
    assert_eq!(
        explicit_parent
            .metadata
            .as_ref()
            .and_then(|m| m.get("type"))
            .and_then(|v| v.as_str()),
        Some("lambda")
    );

    let bare = parameters
        .iter()
        .find(|s| s.name == "simpleParam")
        .expect("bare lambda parameter must be a symbol");
    let bare_parent = symbols
        .iter()
        .find(|s| Some(s.id.as_str()) == bare.parent_id.as_deref())
        .expect("bare lambda parameter must have a parent");
    assert_eq!(
        bare_parent
            .metadata
            .as_ref()
            .and_then(|m| m.get("type"))
            .and_then(|v| v.as_str()),
        Some("lambda")
    );
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

#[test]
fn foreach_tuple_deconstruction_emits_each_binding() {
    let source = r#"
namespace App {
  public class Service {
    public void Run() {
      foreach (var (a, b) in pairs) {
        _ = a;
        _ = b;
      }
      foreach ((int x, string y) in pairs) {
        _ = x;
        _ = y;
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
    for name in ["a", "b", "x", "y"] {
        assert!(
            locals.contains(&name),
            "foreach deconstruction must emit local {name}: {locals:?}"
        );
    }
    let x = symbols.iter().find(|s| s.name == "x").unwrap();
    assert_eq!(
        x.metadata
            .as_ref()
            .and_then(|m| m.get("variableType"))
            .and_then(|v| v.as_str()),
        Some("int")
    );
}

#[test]
fn foreach_var_element_deconstruction_emits_each_binding_once() {
    let source = r#"
namespace App {
  public class Service {
    public void Run() {
      foreach ((var x, var y) in pairs) {
        _ = x;
        _ = y;
      }
    }
  }
}
"#;
    let (symbols, _) = extract(source);
    let x_count = symbols.iter().filter(|s| s.name == "x").count();
    let y_count = symbols.iter().filter(|s| s.name == "y").count();
    assert_eq!(x_count, 1, "foreach (var x, …) must emit x exactly once");
    assert_eq!(y_count, 1, "foreach (…, var y) must emit y exactly once");
}
