use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, StructuralFact, SymbolKind,
};
use crate::pipeline::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn extract(file_path: &str, source: &str) -> ExtractionResults {
    extract_canonical(file_path, source, &PathBuf::from("/tmp/test")).unwrap()
}

fn names(results: &ExtractionResults) -> Vec<&str> {
    results.symbols.iter().map(|s| s.name.as_str()).collect()
}

fn dependency_summary(results: &ExtractionResults) -> BTreeSet<(String, String, String)> {
    facts_with_pattern(results, "manifest.dependency.v1")
        .iter()
        .map(|fact: &&StructuralFact| {
            (
                metadata_str(fact, "name").unwrap().to_string(),
                metadata_str(fact, "group").unwrap().to_string(),
                metadata_str(fact, "version").unwrap_or("").to_string(),
            )
        })
        .collect()
}

fn pending_paths(results: &ExtractionResults) -> BTreeSet<String> {
    results
        .structured_pending_relationships
        .iter()
        .map(|p| p.target.import_context.clone().unwrap())
        .collect()
}

fn dep(name: &str, group: &str, version: &str) -> (String, String, String) {
    (name.to_string(), group.to_string(), version.to_string())
}

const CSPROJ: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <IsTestProject>true</IsTestProject>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="xunit" Version="2.9.3" />
    <ProjectReference Include="..\Core\Core.csproj" />
  </ItemGroup>
  <Import Project="..\build\Common.targets" />
  <UsingTask TaskName="GenerateDocs" AssemblyFile="Tasks.dll" />
  <Target Name="GenerateVersion" DependsOnTargets="Restore;ComputeVersion" BeforeTargets="Build">
    <Message Text="Version $(Version)" />
    <CallTarget Targets="WriteVersionFile" />
  </Target>
  <Target Name="ComputeVersion" />
  <Target Name="WriteVersionFile" />
</Project>
"#;

#[test]
fn msbuild_projects_emit_named_targets_and_a_project_symbol() {
    let results = extract("src/App/App.csproj", CSPROJ);

    assert_eq!(
        names(&results),
        vec![
            "App",
            "GenerateDocs",
            "GenerateVersion",
            "ComputeVersion",
            "WriteVersionFile"
        ]
    );
    let project = &results.symbols[0];
    assert_eq!(project.kind, SymbolKind::Module);
    assert!(
        results.symbols[1..]
            .iter()
            .all(|symbol| symbol.parent_id.as_ref() == Some(&project.id))
    );
    let target = &results.symbols[2];
    let metadata = target.metadata.as_ref().unwrap();
    assert_eq!(metadata["name_attribute"], "Name");
    assert_eq!(metadata["tag"], "Target");
}

#[test]
fn msbuild_targets_call_each_other_and_properties_are_identifiers() {
    let results = extract("App.csproj", CSPROJ);

    let name = |id: &str| {
        results
            .symbols
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .name
            .as_str()
    };
    let calls: BTreeSet<_> = results
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .map(|r| (name(&r.from_symbol_id), name(&r.to_symbol_id)))
        .collect();
    assert_eq!(
        calls,
        BTreeSet::from([
            ("GenerateVersion", "ComputeVersion"),
            ("GenerateVersion", "WriteVersionFile"),
        ])
    );
    let call_names: BTreeSet<_> = results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::Call)
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(
        call_names,
        BTreeSet::from(["Restore", "ComputeVersion", "Build", "WriteVersionFile"])
    );
    let version = results
        .identifiers
        .iter()
        .find(|i| i.kind == IdentifierKind::VariableRef && i.name == "Version")
        .expect("$(Version) should be a variable_ref identifier");
    assert_eq!(
        &CSPROJ[version.start_byte as usize..version.end_byte as usize],
        "Version"
    );
}

#[test]
fn msbuild_dependencies_properties_and_file_references() {
    let results = extract("App.csproj", CSPROJ);

    assert_eq!(
        dependency_summary(&results),
        BTreeSet::from([dep("xunit", "PackageReference", "2.9.3")])
    );
    let properties: BTreeSet<_> = facts_with_pattern(&results, "xml.msbuild_property.v1")
        .iter()
        .map(|f| {
            (
                metadata_str(f, "name").unwrap().to_string(),
                metadata_str(f, "value").unwrap().to_string(),
            )
        })
        .collect();
    assert!(properties.contains(&("TargetFramework".to_string(), "net8.0".to_string())));
    assert!(properties.contains(&("IsTestProject".to_string(), "true".to_string())));
    assert_eq!(
        pending_paths(&results),
        BTreeSet::from([
            "../Core/Core.csproj".to_string(),
            "../build/Common.targets".to_string(),
        ])
    );
    assert!(
        results
            .structured_pending_relationships
            .iter()
            .all(|p| p.pending.kind == RelationshipKind::Imports)
    );
}

#[test]
fn slnx_folders_are_symbols_and_projects_are_pending_references() {
    let source = r#"<Solution>
  <Folder Name="/src/">
    <Project Path="src/App/App.csproj" />
  </Folder>
</Solution>
"#;
    let results = extract("App.slnx", source);

    assert_eq!(names(&results), vec!["App", "/src/"]);
    assert_eq!(
        pending_paths(&results),
        BTreeSet::from(["src/App/App.csproj".to_string()])
    );
}

#[test]
fn nuspec_names_the_package_and_keeps_dependencies_as_facts() {
    let source = r#"<package>
  <metadata>
    <id>Contoso.Core</id>
    <version>1.2.0</version>
    <dependencies>
      <group targetFramework="net8.0">
        <dependency id="Newtonsoft.Json" version="13.0.3" />
      </group>
    </dependencies>
  </metadata>
</package>
"#;
    let results = extract("Contoso.Core.nuspec", source);

    assert_eq!(names(&results), vec!["Contoso.Core"]);
    let facts = facts_with_pattern(&results, "manifest.dependency.v1");
    assert_eq!(facts.len(), 1);
    assert_eq!(metadata_str(facts[0], "name"), Some("Newtonsoft.Json"));
    assert_eq!(metadata_str(facts[0], "target"), Some("net8.0"));
    assert_eq!(metadata_str(facts[0], "ecosystem"), Some("nuget"));
}

#[test]
fn maven_pom_emits_coordinates_dependencies_modules_and_profiles() {
    let source = r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <parent><groupId>com.example</groupId><artifactId>shop-parent</artifactId><version>1.0.0</version></parent>
  <artifactId>shop-api</artifactId>
  <modules><module>shop-core</module></modules>
  <dependencies>
    <dependency><groupId>org.junit.jupiter</groupId><artifactId>junit-jupiter</artifactId><version>5.10.0</version><scope>test</scope></dependency>
  </dependencies>
  <build><plugins><plugin><artifactId>maven-surefire-plugin</artifactId></plugin></plugins></build>
  <profiles><profile><id>ci</id></profile></profiles>
</project>
"#;
    let results = extract("shop-api/pom.xml", source);

    assert_eq!(names(&results), vec!["shop-api", "ci"]);
    let project = results.symbols[0].metadata.as_ref().unwrap();
    assert_eq!(project["groupId"], "com.example");
    assert_eq!(project["version"], "1.0.0");
    assert_eq!(
        dependency_summary(&results),
        BTreeSet::from([
            dep("org.junit.jupiter:junit-jupiter", "test", "5.10.0"),
            dep(
                "org.apache.maven.plugins:maven-surefire-plugin",
                "plugin",
                ""
            ),
        ])
    );
    assert_eq!(
        pending_paths(&results),
        BTreeSet::from(["shop-core/pom.xml".to_string(), "../pom.xml".to_string()])
    );
}
