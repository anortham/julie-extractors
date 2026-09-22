//! Build-manifest dialects of XML: MSBuild projects (`.csproj`, `.vbproj`,
//! `.fsproj`, `.props`, `.targets`), `.slnx` solutions, NuGet `.nuspec`
//! packages, and Maven `pom.xml` files, plus the file references of XSD and
//! WSDL documents.
//!
//! The dialect comes from the file name. Each helper reads the tree and the
//! source text only, so the extractor and the structural-fact collector share
//! one reading of the document.

use tree_sitter::{Node, Tree};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildDialect {
    MsBuild,
    Solution,
    Nuspec,
    Maven,
    Schema,
}

impl BuildDialect {
    pub(crate) fn for_path(file_path: &str) -> Option<Self> {
        let name = file_path.rsplit(['/', '\\']).next()?;
        if name == "pom.xml" {
            return Some(Self::Maven);
        }
        let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
        match extension.as_str() {
            "csproj" | "vbproj" | "fsproj" | "props" | "targets" => Some(Self::MsBuild),
            "slnx" => Some(Self::Solution),
            "nuspec" => Some(Self::Nuspec),
            "xsd" | "wsdl" => Some(Self::Schema),
            _ => None,
        }
    }

    pub(crate) fn ecosystem(self) -> &'static str {
        match self {
            Self::Maven => "maven",
            _ => "nuget",
        }
    }
}

pub(crate) fn root_element(tree: &Tree) -> Option<Node<'_>> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .find(|child| child.kind() == "element")
}

pub(crate) fn tag<'tree>(element: Node<'tree>) -> Option<Node<'tree>> {
    let mut cursor = element.walk();
    element
        .children(&mut cursor)
        .find(|child| matches!(child.kind(), "STag" | "EmptyElemTag"))
}

pub(crate) fn local_tag<'a>(content: &'a str, element: Node<'_>) -> Option<&'a str> {
    let tag = tag(element)?;
    let mut cursor = tag.walk();
    let name = tag
        .children(&mut cursor)
        .find(|child| child.kind() == "Name")?;
    let name = content.get(name.byte_range())?;
    Some(name.rsplit(':').next().unwrap_or(name))
}

/// The `AttValue` node and unquoted value of an attribute, matched by local
/// name without case (MSBuild writes `Include`, `Name`, `Version`).
pub(crate) fn attribute<'tree>(
    content: &str,
    element: Node<'tree>,
    wanted: &str,
) -> Option<(Node<'tree>, String)> {
    let tag = tag(element)?;
    let mut cursor = tag.walk();
    for attribute in tag.children(&mut cursor) {
        if attribute.kind() != "Attribute" {
            continue;
        }
        let mut parts = attribute.walk();
        let mut name = None;
        let mut value = None;
        for part in attribute.children(&mut parts) {
            match part.kind() {
                "Name" if name.is_none() => name = content.get(part.byte_range()),
                "AttValue" => value = Some(part),
                _ => {}
            }
        }
        let (Some(name), Some(value)) = (name, value) else {
            continue;
        };
        if name
            .rsplit(':')
            .next()
            .unwrap_or(name)
            .eq_ignore_ascii_case(wanted)
        {
            let text = content.get(value.byte_range())?;
            let text = text.get(1..text.len().saturating_sub(1))?.trim();
            return (!text.is_empty()).then(|| (value, text.to_string()));
        }
    }
    None
}

pub(crate) fn child_elements(element: Node<'_>) -> Vec<Node<'_>> {
    let mut children = Vec::new();
    let mut cursor = element.walk();
    for child in element.children(&mut cursor) {
        if child.kind() == "content" {
            let mut content_cursor = child.walk();
            children.extend(
                child
                    .children(&mut content_cursor)
                    .filter(|node| node.kind() == "element"),
            );
        }
    }
    children
}

fn child<'tree>(content: &str, element: Node<'tree>, wanted: &str) -> Option<Node<'tree>> {
    child_elements(element)
        .into_iter()
        .find(|child| local_tag(content, *child) == Some(wanted))
}

/// The trimmed character data directly inside an element.
pub(crate) fn text(content: &str, element: Node<'_>) -> Option<String> {
    let mut cursor = element.walk();
    let body = element
        .children(&mut cursor)
        .find(|child| child.kind() == "content")?;
    let mut body_cursor = body.walk();
    let text: String = body
        .children(&mut body_cursor)
        .filter(|node| node.kind() == "CharData")
        .filter_map(|node| content.get(node.byte_range()))
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn child_text(content: &str, element: Node<'_>, wanted: &str) -> Option<String> {
    child(content, element, wanted).and_then(|child| text(content, child))
}

fn elements<'tree>(root: Node<'tree>, out: &mut Vec<Node<'tree>>) {
    out.push(root);
    for child in child_elements(root) {
        elements(child, out);
    }
}

fn all_elements(root: Node<'_>) -> Vec<Node<'_>> {
    let mut out = Vec::new();
    elements(root, &mut out);
    out
}

fn parent_element(element: Node<'_>) -> Option<Node<'_>> {
    std::iter::successors(element.parent(), |node| node.parent())
        .find(|node| node.kind() == "element")
}

fn file_stem(file_path: &str) -> String {
    let name = file_path.rsplit(['/', '\\']).next().unwrap_or(file_path);
    name.rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_string()
}

/// The symbol of a manifest's root element: the MSBuild project name (file
/// stem), the NuGet package id, or the Maven artifactId.
pub(crate) struct DocumentSymbol {
    pub name: String,
    /// Where the name comes from, recorded as `name_attribute`.
    pub source: &'static str,
    pub metadata: Vec<(&'static str, String)>,
}

pub(crate) fn document_symbol(
    dialect: BuildDialect,
    content: &str,
    file_path: &str,
    root: Node<'_>,
) -> Option<DocumentSymbol> {
    let expected_root = match dialect {
        BuildDialect::MsBuild => "Project",
        BuildDialect::Solution => "Solution",
        BuildDialect::Nuspec => "package",
        BuildDialect::Maven => "project",
        BuildDialect::Schema => return None,
    };
    if local_tag(content, root) != Some(expected_root) {
        return None;
    }
    match dialect {
        BuildDialect::MsBuild | BuildDialect::Solution => Some(DocumentSymbol {
            name: file_stem(file_path),
            source: "file_name",
            metadata: Vec::new(),
        }),
        BuildDialect::Nuspec => {
            let metadata = child(content, root, "metadata");
            let id = metadata.and_then(|metadata| child_text(content, metadata, "id"));
            let version = metadata.and_then(|metadata| child_text(content, metadata, "version"));
            Some(DocumentSymbol {
                name: id.unwrap_or_else(|| file_stem(file_path)),
                source: "id",
                metadata: version.map(|v| ("version", v)).into_iter().collect(),
            })
        }
        BuildDialect::Maven => {
            let parent = child(content, root, "parent");
            let inherited = |field: &str| {
                child_text(content, root, field)
                    .or_else(|| parent.and_then(|parent| child_text(content, parent, field)))
            };
            let artifact_id = child_text(content, root, "artifactId")?;
            let extra = [
                ("groupId", inherited("groupId")),
                ("version", inherited("version")),
            ]
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key, value)))
            .collect();
            Some(DocumentSymbol {
                name: artifact_id,
                source: "artifactId",
                metadata: extra,
            })
        }
        BuildDialect::Schema => None,
    }
}

/// NuGet `<dependency id="…">` names a package this one depends on, not a
/// declaration, so its `id` attribute does not promote it to a symbol.
pub(crate) fn is_reference_element(
    dialect: BuildDialect,
    content: &str,
    element: Node<'_>,
) -> bool {
    dialect == BuildDialect::Nuspec && local_tag(content, element) == Some("dependency")
}

/// A Maven `<profile>` is named by its `<id>` child element.
pub(crate) fn child_named_element(
    dialect: BuildDialect,
    content: &str,
    element: Node<'_>,
) -> Option<String> {
    (dialect == BuildDialect::Maven && local_tag(content, element) == Some("profile"))
        .then(|| child_text(content, element, "id"))
        .flatten()
}

pub(crate) struct BuildDependency<'tree> {
    pub node: Node<'tree>,
    pub name: String,
    pub group: String,
    pub version: Option<String>,
    pub target: Option<String>,
}

pub(crate) fn dependencies<'tree>(
    dialect: BuildDialect,
    content: &str,
    root: Node<'tree>,
) -> Vec<BuildDependency<'tree>> {
    let mut found = Vec::new();
    for element in all_elements(root) {
        let Some(tag) = local_tag(content, element) else {
            continue;
        };
        let dependency = match (dialect, tag) {
            (
                BuildDialect::MsBuild,
                "PackageReference" | "PackageVersion" | "GlobalPackageReference",
            ) => attribute(content, element, "Include")
                .or_else(|| attribute(content, element, "Update"))
                .map(|(_, name)| BuildDependency {
                    node: element,
                    name,
                    group: tag.to_string(),
                    version: attribute(content, element, "Version")
                        .or_else(|| attribute(content, element, "VersionOverride"))
                        .map(|(_, version)| version)
                        .or_else(|| child_text(content, element, "Version")),
                    target: None,
                }),
            (BuildDialect::Nuspec, "dependency") => {
                attribute(content, element, "id").map(|(_, name)| BuildDependency {
                    node: element,
                    name,
                    group: "dependency".to_string(),
                    version: attribute(content, element, "version").map(|(_, v)| v),
                    target: parent_element(element)
                        .and_then(|group| attribute(content, group, "targetFramework"))
                        .map(|(_, framework)| framework),
                })
            }
            (BuildDialect::Maven, "dependency" | "plugin") => {
                maven_dependency(content, element, tag)
            }
            _ => None,
        };
        found.extend(dependency);
    }
    found
}

fn maven_dependency<'tree>(
    content: &str,
    element: Node<'tree>,
    tag: &str,
) -> Option<BuildDependency<'tree>> {
    let artifact_id = child_text(content, element, "artifactId")?;
    let group_id = child_text(content, element, "groupId")
        .or_else(|| (tag == "plugin").then(|| "org.apache.maven.plugins".to_string()))?;
    let managed = std::iter::successors(parent_element(element), |node| parent_element(*node)).any(
        |ancestor| {
            matches!(
                local_tag(content, ancestor),
                Some("dependencyManagement" | "pluginManagement")
            )
        },
    );
    let group = match (tag, managed) {
        ("plugin", false) => "plugin".to_string(),
        ("plugin", true) => "managed-plugin".to_string(),
        (_, true) => "managed".to_string(),
        _ => child_text(content, element, "scope").unwrap_or_else(|| "compile".to_string()),
    };
    Some(BuildDependency {
        node: element,
        name: format!("{group_id}:{artifact_id}"),
        group,
        version: child_text(content, element, "version"),
        target: None,
    })
}

/// A reference to another file: the element, the `/`-joined path, and the
/// reference kind (`project_reference`, `import`, `module`, `parent`, ...).
pub(crate) struct FileReference<'tree> {
    pub node: Node<'tree>,
    pub path: String,
    pub kind: &'static str,
}

pub(crate) fn file_references<'tree>(
    dialect: BuildDialect,
    content: &str,
    root: Node<'tree>,
) -> Vec<FileReference<'tree>> {
    let mut found = Vec::new();
    let mut push = |node, path: String, kind| {
        if !path.is_empty() && !path.contains("$(") && !path.contains("://") {
            found.push(FileReference {
                node,
                path: path.replace('\\', "/"),
                kind,
            });
        }
    };
    for element in all_elements(root) {
        let Some(tag) = local_tag(content, element) else {
            continue;
        };
        match (dialect, tag) {
            (BuildDialect::MsBuild, "ProjectReference") => {
                if let Some((_, path)) = attribute(content, element, "Include") {
                    push(element, path, "project_reference");
                }
            }
            (BuildDialect::MsBuild, "Import") => {
                if let Some((_, path)) = attribute(content, element, "Project") {
                    push(element, path, "import");
                }
            }
            (BuildDialect::Solution, "Project") => {
                if let Some((_, path)) = attribute(content, element, "Path") {
                    push(element, path, "project_reference");
                }
            }
            (BuildDialect::Maven, "module") => {
                if let Some(module) = text(content, element) {
                    push(
                        element,
                        format!("{}/pom.xml", module.trim_end_matches('/')),
                        "module",
                    );
                }
            }
            (BuildDialect::Maven, "parent")
                if parent_element(element).is_some_and(|parent| parent.id() == root.id()) =>
            {
                let path = child_text(content, element, "relativePath")
                    .unwrap_or_else(|| "../pom.xml".to_string());
                let path = if path.ends_with(".xml") {
                    path
                } else {
                    format!("{}/pom.xml", path.trim_end_matches('/'))
                };
                push(element, path, "parent");
            }
            (BuildDialect::Schema, "import" | "include" | "redefine") => {
                if let Some((_, path)) = attribute(content, element, "schemaLocation")
                    .or_else(|| attribute(content, element, "location"))
                {
                    let kind = match tag {
                        "import" => "import",
                        "include" => "include",
                        _ => "redefine",
                    };
                    push(element, path, kind);
                }
            }
            _ => {}
        }
    }
    found
}

/// MSBuild `<PropertyGroup>` children: element, property name, and value.
pub(crate) fn properties<'tree>(
    content: &str,
    root: Node<'tree>,
) -> Vec<(Node<'tree>, String, Option<String>)> {
    all_elements(root)
        .into_iter()
        .filter(|element| local_tag(content, *element) == Some("PropertyGroup"))
        .flat_map(child_elements)
        .filter_map(|property| {
            let name = local_tag(content, property)?.to_string();
            Some((property, name, text(content, property)))
        })
        .collect()
}

/// A name inside an attribute value, with its absolute byte range.
pub(crate) struct NameSite {
    pub start: usize,
    pub end: usize,
    pub name: String,
    pub element: Option<usize>,
}

/// Target names in `DependsOnTargets`, `BeforeTargets`, `AfterTargets`, and
/// `<CallTarget Targets>`, with the id of the declaring `<Target>` element.
pub(crate) fn target_references(content: &str, root: Node<'_>) -> Vec<NameSite> {
    let mut sites = Vec::new();
    for element in all_elements(root) {
        let tag = local_tag(content, element);
        let (attributes, owner): (&[&str], Option<Node>) = match tag {
            Some("Target") => (
                &["DependsOnTargets", "BeforeTargets", "AfterTargets"],
                Some(element),
            ),
            Some("CallTarget") => (
                &["Targets"],
                std::iter::successors(parent_element(element), |node| parent_element(*node))
                    .find(|node| local_tag(content, *node) == Some("Target")),
            ),
            _ => continue,
        };
        for attribute_name in attributes {
            let Some((value, _)) = attribute(content, element, attribute_name) else {
                continue;
            };
            let start = value.start_byte() + 1;
            let raw = content
                .get(start..value.end_byte().saturating_sub(1))
                .unwrap_or("");
            let mut offset = 0;
            for part in raw.split(';') {
                let name = part.trim();
                let lead = part.len() - part.trim_start().len();
                if !name.is_empty() && !name.contains("$(") && !name.contains("@(") {
                    sites.push(NameSite {
                        start: start + offset + lead,
                        end: start + offset + lead + name.len(),
                        name: name.to_string(),
                        element: owner.map(|owner| owner.id()),
                    });
                }
                offset += part.len() + 1;
            }
        }
    }
    sites
}

/// `$(Name)` property references in attribute values and element text.
pub(crate) fn property_references(content: &str, root: Node<'_>) -> Vec<NameSite> {
    let mut sites = Vec::new();
    for element in all_elements(root) {
        let mut texts = Vec::new();
        if let Some(tag) = tag(element) {
            let mut cursor = tag.walk();
            for attribute in tag.children(&mut cursor) {
                let mut parts = attribute.walk();
                texts.extend(
                    attribute
                        .children(&mut parts)
                        .filter(|part| part.kind() == "AttValue"),
                );
            }
        }
        let mut cursor = element.walk();
        for body in element
            .children(&mut cursor)
            .filter(|child| child.kind() == "content")
        {
            let mut body_cursor = body.walk();
            texts.extend(
                body.children(&mut body_cursor)
                    .filter(|node| node.kind() == "CharData"),
            );
        }
        for node in texts {
            scan_property_references(content, node.start_byte(), node.end_byte(), &mut sites);
        }
    }
    sites
}

fn scan_property_references(content: &str, from: usize, to: usize, sites: &mut Vec<NameSite>) {
    let Some(text) = content.get(from..to) else {
        return;
    };
    let mut search = 0;
    while let Some(found) = text[search..].find("$(") {
        let start = search + found + 2;
        let end = text[start..]
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .map_or(text.len(), |index| start + index);
        if end > start && text[end..].starts_with(')') {
            sites.push(NameSite {
                start: from + start,
                end: from + end,
                name: text[start..end].to_string(),
                element: None,
            });
        }
        search = end.max(start);
    }
}

pub(crate) const MSBUILD_PROPERTY_PATTERN_ID: &str = "xml.msbuild_property.v1";

/// `manifest.dependency.v1` facts for package dependencies and
/// `xml.msbuild_property.v1` facts for MSBuild properties.
pub(crate) fn build_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<crate::base::StructuralFact> {
    use crate::base::structural_fact_builders::{base_metadata, fact_for_node, insert_string};

    let (Some(dialect), Some(root)) = (BuildDialect::for_path(file_path), root_element(tree))
    else {
        return Vec::new();
    };
    let mut facts = Vec::new();
    for dependency in dependencies(dialect, content, root) {
        let mut metadata = base_metadata("dependencies");
        insert_string(&mut metadata, "ecosystem", dialect.ecosystem());
        insert_string(&mut metadata, "name", &dependency.name);
        insert_string(&mut metadata, "group", &dependency.group);
        for (key, value) in [
            ("version", &dependency.version),
            ("target", &dependency.target),
        ] {
            if let Some(value) = value {
                insert_string(&mut metadata, key, value);
            }
        }
        facts.push(fact_for_node(
            file_path,
            "xml",
            crate::toml::dependencies::MANIFEST_DEPENDENCY_PATTERN_ID,
            "dependency",
            dependency.node,
            metadata,
        ));
    }
    if dialect == BuildDialect::MsBuild {
        for (node, name, value) in properties(content, root) {
            let mut metadata = base_metadata("config_structure");
            insert_string(&mut metadata, "name", &name);
            if let Some(value) = value {
                insert_string(&mut metadata, "value", &value);
            }
            if let Some((_, condition)) = attribute(content, node, "Condition") {
                insert_string(&mut metadata, "condition", &condition);
            }
            facts.push(fact_for_node(
                file_path,
                "xml",
                MSBUILD_PROPERTY_PATTERN_ID,
                "property",
                node,
                metadata,
            ));
        }
    }
    facts
}
