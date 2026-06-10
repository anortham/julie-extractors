use super::{CSharpExtractor, SymbolKind, init_parser};
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_return_type_ignores_method_name_in_attribute_argument_default() {
        let code = r#"
namespace Example
{
    public class GetNameHandler {}

    public class Handler
    {
        [CustomRoute(Name = "GetName")]
        public string GetName(string fallback = "GetName") => fallback;

        public GetNameHandler Name() => null;
    }
}
"#;

        let mut parser = init_parser();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CSharpExtractor::new(
            "c_sharp".to_string(),
            "type_inference.cs".to_string(),
            code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let types = extractor.infer_types(&symbols);

        let get_name = symbols
            .iter()
            .find(|symbol| symbol.name == "GetName" && symbol.kind == SymbolKind::Method)
            .expect("GetName method should be extracted");
        assert_eq!(
            types.get(&get_name.id).map(String::as_str),
            Some("string"),
            "return type must come from the method declaration, not attribute/default strings"
        );

        let name = symbols
            .iter()
            .find(|symbol| symbol.name == "Name" && symbol.kind == SymbolKind::Method)
            .expect("Name method should be extracted");
        assert_eq!(
            types.get(&name.id).map(String::as_str),
            Some("GetNameHandler"),
            "return type must not match inside GetNameHandler when method is Name"
        );
    }

    #[test]
    fn method_return_type_regression_does_not_change_existing_methods() {
        let code = r#"
namespace MyProject
{
    public class TypeExample
    {
        public string GetName() => "test";
        public Task<List<User>> GetUsersAsync() => null;
        public void ProcessData<T>(T data) where T : class { }
    }
}
"#;

        let mut parser = init_parser();
        let tree = parser.parse(code, None).unwrap();
        let workspace_root = PathBuf::from("/tmp/test");
        let mut extractor = CSharpExtractor::new(
            "c_sharp".to_string(),
            "test.cs".to_string(),
            code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let types = extractor.infer_types(&symbols);

        let get_name = symbols
            .iter()
            .find(|symbol| symbol.name == "GetName")
            .unwrap();
        assert_eq!(types.get(&get_name.id).unwrap(), "string");

        let get_users = symbols
            .iter()
            .find(|symbol| symbol.name == "GetUsersAsync")
            .unwrap();
        assert_eq!(types.get(&get_users.id).unwrap(), "Task<List<User>>");

        let process_data = symbols
            .iter()
            .find(|symbol| symbol.name == "ProcessData")
            .unwrap();
        assert_eq!(types.get(&process_data.id).unwrap(), "void");
    }
}
