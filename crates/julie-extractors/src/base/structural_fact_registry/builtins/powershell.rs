//! Built-in language-local SPECS for PowerShell.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, ARR, BASE_KEYS, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR, StructuralFactPatternSpec,
    key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "powershell.cmdlet_binding_attribute.v1",
        languages: &["powershell"],
        query_family: "metadata",
        description: "A PowerShell `[CmdletBinding()]` attribute.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "The attribute name (always \"CmdletBinding\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "powershell.param_block.v1",
        languages: &["powershell"],
        query_family: "parameters",
        description: "A PowerShell `param(...)` block.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "powershell.pipeline_expression.v1",
        languages: &["powershell"],
        query_family: "pipeline",
        description: "A PowerShell pipeline expression (`|`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "pipeline_marker",
                STR,
                ALWAYS,
                "Pipeline marker token (always \"|\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "powershell.class_definition.v1",
        languages: &["powershell"],
        query_family: "types",
        description: "A PowerShell `class` definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("class_name", STR, OPT, "The PowerShell class name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "powershell.dsc_resource.v1",
        languages: &["powershell"],
        query_family: "configuration",
        description: "A DSC resource instance inside a `Configuration` block (`WindowsFeature IIS { ... }`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "resource_type",
                STR,
                ALWAYS,
                "The DSC resource type (`WindowsFeature`).",
            ),
            key(
                "resource_name",
                STR,
                ALWAYS,
                "The resource instance name (`IIS`).",
            ),
            key(
                "configuration_name",
                STR,
                ALWAYS,
                "The enclosing `Configuration` name.",
            ),
            key(
                "node_name",
                STR,
                OPT,
                "The argument text of the enclosing `Node` block, when there is one.",
            ),
            key(
                "depends_on",
                ARR,
                OPT,
                "The `DependsOn` references (`[WindowsFeature]IIS`), when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "powershell.data_key.v1",
        languages: &["powershell"],
        query_family: "config_structure",
        description: "A key of a PowerShell data file (`.psd1`) hashtable.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("key", STR, ALWAYS, "The key name."),
            key(
                "key_path",
                STR,
                ALWAYS,
                "Full dotted key path including the enclosing hashtable keys.",
            ),
            key(
                "value_kind",
                STR,
                OPT,
                "Kind of the value: string, number, boolean, array, hashtable, scriptblock, or expression.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "powershell.module_manifest.v1",
        languages: &["powershell"],
        query_family: "module",
        description: "A PowerShell module manifest (`.psd1` hashtable that sets `RootModule` or `ModuleVersion`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("root_module", STR, OPT, "The `RootModule` value."),
            key("module_version", STR, OPT, "The `ModuleVersion` value."),
            key("guid", STR, OPT, "The `GUID` value."),
            key(
                "powershell_version",
                STR,
                OPT,
                "The minimum `PowerShellVersion` value.",
            ),
        ],
    },
];
