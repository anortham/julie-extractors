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

pub(crate) fn is_ecmascript_global_direct_target(name: &str) -> bool {
    matches!(
        name,
        "AggregateError"
            | "AbortController"
            | "Array"
            | "ArrayBuffer"
            | "Atomics"
            | "BigInt"
            | "BigInt64Array"
            | "BigUint64Array"
            | "Blob"
            | "Boolean"
            | "Buffer"
            | "DataView"
            | "Date"
            | "Error"
            | "EvalError"
            | "File"
            | "Float32Array"
            | "Float64Array"
            | "FormData"
            | "Headers"
            | "Int16Array"
            | "Int32Array"
            | "Int8Array"
            | "Map"
            | "Number"
            | "Object"
            | "Promise"
            | "RangeError"
            | "ReferenceError"
            | "Reflect"
            | "RegExp"
            | "Request"
            | "Response"
            | "Set"
            | "String"
            | "Symbol"
            | "SyntaxError"
            | "TextDecoder"
            | "TextEncoder"
            | "TypeError"
            | "URIError"
            | "URL"
            | "URLSearchParams"
            | "Uint16Array"
            | "Uint32Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "WeakMap"
            | "WeakSet"
            | "WebSocket"
            | "clearImmediate"
            | "clearInterval"
            | "clearTimeout"
            | "decodeURI"
            | "decodeURIComponent"
            | "encodeURI"
            | "encodeURIComponent"
            | "eval"
            | "fetch"
            | "import"
            | "isFinite"
            | "isNaN"
            | "parseFloat"
            | "parseInt"
            | "queueMicrotask"
            | "setImmediate"
            | "setInterval"
            | "setTimeout"
            | "structuredClone"
    )
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

    #[test]
    fn classifies_ecmascript_globals_as_external_direct_targets() {
        for name in [
            "Error",
            "String",
            "Boolean",
            "Number",
            "Promise",
            "Set",
            "Map",
            "Date",
            "Response",
            "URL",
            "AbortController",
            "Uint8Array",
            "fetch",
            "setTimeout",
            "clearTimeout",
            "encodeURIComponent",
            "structuredClone",
            "import",
        ] {
            assert!(is_ecmascript_global_direct_target(name));
        }

        for name in ["projectGlobal", "normalizeOptionalString", "createRun"] {
            assert!(!is_ecmascript_global_direct_target(name));
        }
    }
}
