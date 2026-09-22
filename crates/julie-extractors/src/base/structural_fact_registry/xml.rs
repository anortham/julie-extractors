//! Structural-fact pattern SPECS for the `xml` registry family.
//!
//! Authored metadata for [`super::StructuralFactPatternSpec`] entries emitted by
//! the xml arm of `base/data_structural_facts.rs`. Public registry access remains
//! through [`super::structural_fact_pattern_specs`].
//!
//! Several layers share the `xml` language and the `xml.` id prefix: generic
//! document facts (document, namespace declarations, document links, config
//! entries) fire for every registered extension, `xml.xsd.*` for `.xsd` files
//! and schemas inlined in `.wsdl` files, `xml.wsdl.*` only for `.wsdl`,
//! `xml.msbuild_*` only for MSBuild project files, and the framework facts
//! (Spring, servlet, Android, MyBatis, TestNG) only for documents of that
//! framework. QName-valued keys carry the raw prefixed text; namespace
//! resolution lives in the identifier and relationship rows.

use super::{
    ALWAYS, ARR, BOOL, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR,
    StructuralFactPatternSpec, key,
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
                "target_namespace",
                STR,
                OPT,
                "The root element's `targetNamespace`, for schema and service documents.",
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
        pattern_id: "xml.xsd.schema.v1",
        languages: &["xml"],
        query_family: "schema_structure",
        description: "An XSD `schema` element, in a `.xsd` file or inline in a WSDL `types` section.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "target_namespace",
                STR,
                OPT,
                "Declared `targetNamespace`, which `tns:`-style QNames resolve against.",
            ),
            key(
                "element_form_default",
                STR,
                OPT,
                "Declared `elementFormDefault`.",
            ),
            key(
                "attribute_form_default",
                STR,
                OPT,
                "Declared `attributeFormDefault`.",
            ),
            key("version", STR, OPT, "Declared schema `version`."),
        ],
    },
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
        description: "A WSDL 1.1 `port` or WSDL 2.0 `endpoint` declaration inside a service.",
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
            key(
                "address_location",
                STR,
                OPT,
                "Endpoint URL: the `location` of a SOAP or HTTP `address` child, or a WSDL 2.0 `address`.",
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
    StructuralFactPatternSpec {
        pattern_id: "xml.msbuild_property.v1",
        languages: &["xml"],
        query_family: "config_structure",
        description: "An MSBuild property: a child element of `<PropertyGroup>`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("name", STR, ALWAYS, "Property name (the element name)."),
            key("value", STR, OPT, "Trimmed property text."),
            key(
                "condition",
                STR,
                OPT,
                "The property's `Condition` attribute.",
            ),
        ],
    },
    // -----------------------------------------------------------------------
    // Links, configuration entries, and framework vocabularies.
    // -----------------------------------------------------------------------
    StructuralFactPatternSpec {
        pattern_id: "xml.document_link.v1",
        languages: &["xml"],
        query_family: "document_links",
        description: "A link to another file: a stylesheet or xml-model processing instruction, the DOCTYPE system id, an external entity, an XInclude, an `xsi` schema location, or an XSLT import or include.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("href", STR, ALWAYS, "Linked location as written."),
            key(
                "link_kind",
                STR,
                ALWAYS,
                "\"stylesheet\", \"xml_model\", \"dtd\", \"external_entity\", \"xinclude\", \"schema_location\", \"no_namespace_schema_location\", \"xsl_import\", or \"xsl_include\".",
            ),
            key(
                "namespace",
                STR,
                OPT,
                "Namespace paired with the location in `xsi:schemaLocation`.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.config_entry.v1",
        languages: &["xml"],
        query_family: "config_structure",
        description: "A `<add key=\"…\" value=\"…\"/>` configuration entry (.NET `appSettings` and similar sections).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("key", STR, ALWAYS, "The entry's `key`."),
            key("value", STR, OPT, "The entry's `value`."),
            key(
                "section",
                STR,
                OPT,
                "Local name of the enclosing section element.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.spring_bean.v1",
        languages: &["xml"],
        query_family: "framework",
        description: "A Spring `<bean>` definition in a `<beans>` document.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "bean_id",
                STR,
                OPT,
                "Declared `id`; absent on an inner bean.",
            ),
            key("class", STR, OPT, "Qualified bean class."),
            key("scope", STR, OPT, "Declared `scope`."),
            key("init_method", STR, OPT, "Declared `init-method`."),
            key("destroy_method", STR, OPT, "Declared `destroy-method`."),
            key("factory_method", STR, OPT, "Declared `factory-method`."),
            key("factory_bean", STR, OPT, "Declared `factory-bean`."),
            key("parent", STR, OPT, "Declared `parent` bean."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.spring_component_scan.v1",
        languages: &["xml"],
        query_family: "framework",
        description: "A Spring `<context:component-scan>` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "base_package",
                STR,
                ALWAYS,
                "Scanned `base-package` as written.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.servlet_route.v1",
        languages: &["xml"],
        query_family: "framework",
        description: "A `web.xml` servlet or filter mapping: one fact per `url-pattern`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("mapping_kind", STR, ALWAYS, "\"servlet\" or \"filter\"."),
            key(
                "route_template",
                STR,
                ALWAYS,
                "The `url-pattern` as written.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family normalized route template.",
            ),
            key(
                "target_name",
                STR,
                ALWAYS,
                "The mapped `servlet-name` or `filter-name`.",
            ),
            key(
                "target_class",
                STR,
                OPT,
                "Class of the named servlet or filter, when it is declared in the same file.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.android_component.v1",
        languages: &["xml"],
        query_family: "framework",
        description: "An Android manifest component: application, activity, activity alias, service, receiver, or provider.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("component", STR, ALWAYS, "Component element name."),
            key(
                "class",
                STR,
                ALWAYS,
                "Component class, qualified against the manifest `package` when written relative.",
            ),
            key("exported", BOOL, OPT, "Declared `android:exported`."),
            key(
                "intent_actions",
                ARR,
                OPT,
                "Actions of the component's intent filters.",
            ),
            key(
                "intent_categories",
                ARR,
                OPT,
                "Categories of the component's intent filters.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.android_permission.v1",
        languages: &["xml"],
        query_family: "framework",
        description: "An Android manifest permission the app uses or declares.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("permission", STR, ALWAYS, "Permission name."),
            key(
                "usage",
                STR,
                ALWAYS,
                "\"uses\" for `uses-permission`, \"declares\" for `permission`.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.mybatis_statement.v1",
        languages: &["xml"],
        query_family: "query_structure",
        description: "A MyBatis mapper statement or SQL fragment.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "namespace",
                STR,
                OPT,
                "The mapper `namespace`: the Java interface the statements implement.",
            ),
            key("statement_id", STR, ALWAYS, "Declared statement `id`."),
            key(
                "operation",
                STR,
                ALWAYS,
                "\"select\", \"insert\", \"update\", \"delete\", or \"sql\".",
            ),
            key(
                "sql",
                STR,
                ALWAYS,
                "Statement text, CDATA and dynamic-tag text included, whitespace collapsed.",
            ),
            key("parameter_type", STR, OPT, "Declared `parameterType`."),
            key("result_type", STR, OPT, "Declared `resultType`."),
            key("result_map", STR, OPT, "Declared `resultMap`."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "xml.test_selection.v1",
        languages: &["xml"],
        query_family: "testing",
        description: "A TestNG suite `<class>` entry: the test class a suite runs and its method selection.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("class", STR, ALWAYS, "Qualified test class."),
            key("test", STR, OPT, "Name of the enclosing `<test>`."),
            key("suite", STR, OPT, "Name of the enclosing `<suite>`."),
            key(
                "included_methods",
                ARR,
                OPT,
                "Methods named by `<include>`.",
            ),
            key(
                "excluded_methods",
                ARR,
                OPT,
                "Methods named by `<exclude>`.",
            ),
        ],
    },
];
