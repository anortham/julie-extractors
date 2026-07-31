//! Structural-fact pattern SPECS for the `xml` registry family.
//!
//! Authored metadata for [`super::StructuralFactPatternSpec`] entries emitted by
//! the xml arm of `base/data_structural_facts.rs`. Public registry access remains
//! through [`super::structural_fact_pattern_specs`].
//!
//! Three layers share the `xml` language and the `xml.` id prefix: generic
//! document facts fire for every registered extension, `xml.xsd.*` only for
//! `.xsd`, and `xml.wsdl.*` only for `.wsdl`. QName-valued keys carry the raw
//! prefixed text; the tier performs no namespace resolution.

use super::{
    ALWAYS, BOOL, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    // -----------------------------------------------------------------------
    // Generic document structure (every .xml, .xsd, and .wsdl document).
    // -----------------------------------------------------------------------
    StructuralFactPatternSpec {
        pattern_id: "xml.document.v1",
        languages: &["xml"],
        query_family: "document_structure",
        description: "An XML document with a root element.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "dialect",
                STR,
                ALWAYS,
                "Extension-derived document dialect (\"xml\", \"xsd\", or \"wsdl\").",
            ),
            key(
                "root_element",
                STR,
                ALWAYS,
                "Qualified tag name of the root element, prefix included.",
            ),
            key(
                "has_xml_declaration",
                BOOL,
                ALWAYS,
                "Whether the document opens with an `<?xml …?>` declaration.",
            ),
            key(
                "element_count",
                NUM,
                ALWAYS,
                "Total number of elements in the document.",
            ),
            key(
                "max_depth",
                NUM,
                ALWAYS,
                "Deepest element nesting level, counting the root element as 1.",
            ),
            key(
                "namespace_count",
                NUM,
                ALWAYS,
                "Number of `xmlns` declarations anywhere in the document.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.namespace_declaration.v1",
        languages: &["xml"],
        query_family: "document_metadata",
        description: "An `xmlns` namespace declaration attribute.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "namespace_uri",
                STR,
                ALWAYS,
                "Namespace URI the declaration binds.",
            ),
            key(
                "is_default",
                BOOL,
                ALWAYS,
                "Whether the declaration binds the default namespace (`xmlns=`).",
            ),
            key(
                "prefix",
                STR,
                OPT,
                "Bound prefix; absent on a default-namespace declaration.",
            ),
        ],
    },
    // -----------------------------------------------------------------------
    // XML Schema documents (.xsd).
    // -----------------------------------------------------------------------
    StructuralFactPatternSpec {
        pattern_id: "xml.xsd.type.v1",
        languages: &["xml"],
        query_family: "schema_structure",
        description: "A named XSD `complexType` or `simpleType` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("type_name", STR, ALWAYS, "Declared type name."),
            key(
                "type_kind",
                STR,
                ALWAYS,
                "Declared type flavour (\"complex\" or \"simple\").",
            ),
            key(
                "base_type",
                STR,
                OPT,
                "Raw QName the type restricts or extends, when it derives from another type.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.xsd.element.v1",
        languages: &["xml"],
        query_family: "schema_structure",
        description: "A top-level XSD `element` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("element_name", STR, ALWAYS, "Declared element name."),
            key(
                "type_ref",
                STR,
                OPT,
                "Raw QName of the declared element's type, when it names one.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.xsd.import.v1",
        languages: &["xml"],
        query_family: "schema_structure",
        description: "An XSD `import` or `include` of another schema document.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "import_kind",
                STR,
                ALWAYS,
                "Whether the reference is an \"import\" or an \"include\".",
            ),
            key(
                "schema_location",
                STR,
                OPT,
                "Declared `schemaLocation` of the referenced document.",
            ),
            key(
                "namespace",
                STR,
                OPT,
                "Declared `namespace` of the imported schema; absent on an include.",
            ),
        ],
    },
    // -----------------------------------------------------------------------
    // WSDL service definitions (.wsdl).
    // -----------------------------------------------------------------------
    StructuralFactPatternSpec {
        pattern_id: "xml.wsdl.service.v1",
        languages: &["xml"],
        query_family: "service_structure",
        description: "A WSDL `service` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("service_name", STR, ALWAYS, "Declared service name."),
            key(
                "port_count",
                NUM,
                ALWAYS,
                "Number of ports declared directly under the service.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.wsdl.port.v1",
        languages: &["xml"],
        query_family: "service_structure",
        description: "A WSDL `port` declaration inside a service.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("port_name", STR, ALWAYS, "Declared port name."),
            key(
                "binding",
                STR,
                OPT,
                "Raw QName of the binding the port exposes.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.wsdl.binding.v1",
        languages: &["xml"],
        query_family: "service_structure",
        description: "A WSDL `binding` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("binding_name", STR, ALWAYS, "Declared binding name."),
            key(
                "port_type",
                STR,
                OPT,
                "Raw QName of the port type the binding implements.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.wsdl.message.v1",
        languages: &["xml"],
        query_family: "service_structure",
        description: "A WSDL `message` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("message_name", STR, ALWAYS, "Declared message name."),
            key(
                "part_count",
                NUM,
                ALWAYS,
                "Number of parts declared directly under the message.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.wsdl.operation.v1",
        languages: &["xml"],
        query_family: "service_structure",
        description: "A WSDL `operation` declaration inside a port type or a binding.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("operation_name", STR, ALWAYS, "Declared operation name."),
            key(
                "parent_kind",
                STR,
                OPT,
                "Owning declaration kind (\"port_type\" or \"binding\"), when the operation has one.",
            ),
            key(
                "parent_name",
                STR,
                OPT,
                "Declared name of the owning port type or binding.",
            ),
            key(
                "input_message",
                STR,
                OPT,
                "Raw QName of the operation's input message.",
            ),
            key(
                "output_message",
                STR,
                OPT,
                "Raw QName of the operation's output message.",
            ),
        ],
    },
];
