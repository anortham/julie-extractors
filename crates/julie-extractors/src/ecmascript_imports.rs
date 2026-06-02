use crate::base::Symbol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportSourceKind {
    ProjectRelative,
    External,
}

pub(crate) fn import_source_kind(source: &str) -> ImportSourceKind {
    if is_project_relative_import_source(source) {
        ImportSourceKind::ProjectRelative
    } else {
        ImportSourceKind::External
    }
}

pub(crate) fn import_source_from_symbol(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("source"))
        .and_then(|source| source.as_str())
        .filter(|source| !source.is_empty())
}

fn is_project_relative_import_source(source: &str) -> bool {
    matches!(source, "." | "..")
        || source.starts_with("./")
        || source.starts_with("../")
        || source.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_relative_import_sources_as_project_relative() {
        for source in [".", "..", "./helper", "../shared", "/absolute/project/path"] {
            assert_eq!(
                import_source_kind(source),
                ImportSourceKind::ProjectRelative
            );
        }
    }

    #[test]
    fn classifies_package_node_and_alias_sources_as_external() {
        for source in ["vitest", "node:path", "@app/shared", "openclaw/plugin-sdk"] {
            assert_eq!(import_source_kind(source), ImportSourceKind::External);
        }
    }
}
