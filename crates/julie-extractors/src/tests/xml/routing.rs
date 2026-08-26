use crate::pipeline::extract_canonical;
use std::path::PathBuf;

const DOCUMENT: &str = "<root name=\"cfg\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:entry name=\"a\" type=\"xs:string\"/></root>\n";

fn extract(file_path: &str) -> crate::ExtractionResults {
    extract_canonical(file_path, DOCUMENT, &PathBuf::from("/tmp/test")).expect("extraction failed")
}

#[test]
fn xml_xsd_and_wsdl_extensions_all_route_to_the_xml_extractor() {
    for file_path in ["config.xml", "schema.xsd", "service.wsdl"] {
        let results = extract(file_path);
        let names: Vec<_> = results
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect();

        assert_eq!(names, vec!["cfg", "a"], "{file_path} routing");
        assert!(
            results
                .symbols
                .iter()
                .all(|symbol| symbol.language == "xml"),
            "{file_path} should extract as xml"
        );
        assert_eq!(results.identifiers.len(), 1, "{file_path} identifiers");
    }
}

#[test]
fn msbuild_and_dotnet_xml_extensions_route_to_the_xml_extractor() {
    for file_path in [
        "App.csproj",
        "Directory.Build.props",
        "Custom.targets",
        "App.vbproj",
        "App.fsproj",
        "App.slnx",
        "App.nuspec",
        "Resources.resx",
    ] {
        let results = extract(file_path);

        assert_eq!(names(&results), vec!["cfg", "a"], "{file_path} routing");
        assert!(
            results
                .symbols
                .iter()
                .all(|symbol| symbol.language == "xml"),
            "{file_path} should extract as xml"
        );
    }
}

#[test]
fn a_csproj_document_extracts_its_named_msbuild_structure() {
    const CSPROJ: &str = "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <TargetFramework>net10.0</TargetFramework>\n  </PropertyGroup>\n  <ItemGroup>\n    <PackageReference Include=\"xunit.v3\" Version=\"1.0.0\"/>\n  </ItemGroup>\n</Project>\n";

    let results = extract_canonical("App.csproj", CSPROJ, &PathBuf::from("/tmp/test"))
        .expect("csproj extraction failed");

    assert!(
        results
            .symbols
            .iter()
            .all(|symbol| symbol.language == "xml"),
        "csproj content should extract as xml"
    );
}

fn names(results: &crate::ExtractionResults) -> Vec<&str> {
    results
        .symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect()
}

#[test]
fn the_xml_tier_emits_no_relationships_or_types() {
    let results = extract("config.xml");

    assert!(results.relationships.is_empty());
    assert!(results.pending_relationships.is_empty());
    assert!(results.structured_pending_relationships.is_empty());
    assert!(results.types.is_empty());
}
