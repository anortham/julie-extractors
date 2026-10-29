use crate::base::{ExtractionResults, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("R extraction succeeds")
}

fn imports(result: &ExtractionResults) -> Vec<(String, Option<String>, Option<String>)> {
    result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .map(|symbol| {
            let text = |key: &str| {
                symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get(key))
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            };
            (symbol.name.clone(), text("source"), text("form"))
        })
        .collect()
}

fn row(name: &str, form: &str) -> (String, Option<String>, Option<String>) {
    (
        name.to_string(),
        Some(name.to_string()),
        Some(form.to_string()),
    )
}

#[test]
fn load_calls_carry_source_and_form() {
    let result = extract(
        "R/main.R",
        r#"source("R/utils.R")
source('../shared/helpers.R')
source(file.path("R", "more.R"))
library(dplyr)
library("ggplot2")
require(stringr)
requireNamespace("jsonlite")
pacman::p_load(a, b)
box::use(app/logic/utils, pkg[f])
import::from(magrittr, "%>%")
"#,
    );

    assert_eq!(
        imports(&result),
        vec![
            row("R/utils.R", "source()"),
            row("../shared/helpers.R", "source()"),
            row("R/more.R", "source()"),
            row("dplyr", "library()"),
            row("ggplot2", "library()"),
            row("stringr", "require()"),
            row("jsonlite", "requireNamespace()"),
            row("a", "pacman::p_load()"),
            row("b", "pacman::p_load()"),
            row("app/logic/utils", "box::use()"),
            row("pkg", "box::use()"),
            row("magrittr", "import::from()"),
        ]
    );
}

#[test]
fn dynamic_source_path_writes_no_import_row() {
    let result = extract(
        "R/main.R",
        "source(file.path(dir, \"x.R\"))\nsource(paste0(root, \"/y.R\"))\n",
    );

    assert!(imports(&result).is_empty());
}

#[test]
fn namespace_directives_carry_source_and_form() {
    let result = extract("NAMESPACE", "import(methods)\nimportFrom(utils, head)\n");

    assert_eq!(
        imports(&result),
        vec![row("methods", "import()"), row("utils", "importFrom()")]
    );
}
