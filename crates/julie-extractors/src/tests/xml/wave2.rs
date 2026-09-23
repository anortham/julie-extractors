use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, SourceRegionKind, Symbol, SymbolKind,
};
use crate::pipeline::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn extract(file_path: &str, source: &str) -> ExtractionResults {
    extract_canonical(file_path, source, &PathBuf::from("/tmp/test")).expect("extraction failed")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| {
            let names: Vec<_> = results.symbols.iter().map(|s| s.name.as_str()).collect();
            panic!("no symbol {name}; got {names:?}")
        })
}

fn has_symbol(results: &ExtractionResults, name: &str) -> bool {
    results.symbols.iter().any(|symbol| symbol.name == name)
}

fn name_of<'a>(results: &'a ExtractionResults, id: &str) -> &'a str {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map_or("?", |symbol| symbol.name.as_str())
}

fn edges(results: &ExtractionResults, kind: RelationshipKind) -> BTreeSet<(String, String)> {
    results
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == kind)
        .map(|relationship| {
            (
                name_of(results, &relationship.from_symbol_id).to_string(),
                name_of(results, &relationship.to_symbol_id).to_string(),
            )
        })
        .collect()
}

fn identifiers(results: &ExtractionResults, kind: IdentifierKind) -> Vec<&str> {
    results
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == kind)
        .map(|identifier| identifier.name.as_str())
        .collect()
}

fn pending(results: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, Option<String>)> {
    results
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == kind)
        .map(|pending| {
            (
                pending.target.terminal_name.clone(),
                pending.target.import_context.clone(),
            )
        })
        .collect()
}

fn fact_symbol<'a>(
    results: &'a ExtractionResults,
    pattern: &str,
    key: &str,
    value: &str,
) -> &'a str {
    let fact = facts_with_pattern(results, pattern)
        .into_iter()
        .find(|fact| metadata_str(fact, key) == Some(value))
        .unwrap_or_else(|| panic!("no {pattern} fact with {key}={value}"));
    fact.containing_symbol_id
        .as_deref()
        .map_or("", |id| name_of(results, id))
}

const ORDERS_XSD: &str = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:tns="urn:example:orders"
           xmlns:common="urn:example:common"
           targetNamespace="urn:example:orders">
  <xs:import namespace="urn:example:common" schemaLocation="common.xsd"/>
  <xs:complexType name="Order">
    <xs:annotation><xs:documentation>A customer order with one or more line items.</xs:documentation></xs:annotation>
    <xs:complexContent>
      <xs:extension base="tns:BaseEntity">
        <xs:sequence>
          <xs:element name="shipTo" type="tns:Address"/>
          <xs:element name="status" type="tns:OrderStatus"/>
          <xs:element name="money" type="common:Money"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
  <xs:complexType name="BaseEntity"/>
  <xs:complexType name="Address"/>

  <!-- Status of an order in its lifecycle. -->
  <xs:simpleType name="OrderStatus">
    <xs:restriction base="xs:string">
      <xs:enumeration value="Pending"/>
      <xs:enumeration value="Shipped"/>
    </xs:restriction>
  </xs:simpleType>
  <xs:simpleType name="Sku"><xs:restriction base="xs:string"/></xs:simpleType>
  <xs:simpleType name="Skus"><xs:list itemType="tns:Sku"/></xs:simpleType>
  <xs:simpleType name="Code"><xs:union memberTypes="tns:Sku  tns:OrderStatus"/></xs:simpleType>
  <xs:element name="Vehicle" type="tns:Address" abstract="true"/>
  <xs:element name="Car" type="tns:Address" substitutionGroup="tns:Vehicle"/>
  <xs:element name="Catalog">
    <xs:complexType>
      <xs:sequence><xs:element ref="tns:Car"/></xs:sequence>
    </xs:complexType>
    <xs:key name="ItemKey"><xs:selector xpath="item"/><xs:field xpath="@id"/></xs:key>
    <xs:keyref name="ItemRef" refer="tns:ItemKey"><xs:selector xpath="ref"/><xs:field xpath="@id"/></xs:keyref>
  </xs:element>
</xs:schema>
"#;

#[test]
fn xsd_documentation_and_leading_comments_become_doc_comments() {
    let results = extract("orders.xsd", ORDERS_XSD);

    assert_eq!(
        symbol(&results, "Order").doc_comment.as_deref(),
        Some("A customer order with one or more line items.")
    );
    assert_eq!(
        symbol(&results, "OrderStatus").doc_comment.as_deref(),
        Some("<!-- Status of an order in its lifecycle. -->")
    );
    assert_eq!(symbol(&results, "Address").doc_comment, None);
    assert!(
        results
            .source_regions
            .iter()
            .any(|region| region.kind == SourceRegionKind::DocComment)
    );
}

#[test]
fn xsd_components_take_their_declared_kind_in_every_form() {
    let results = extract("orders.xsd", ORDERS_XSD);

    assert_eq!(symbol(&results, "Order").kind, SymbolKind::Class);
    assert_eq!(symbol(&results, "BaseEntity").kind, SymbolKind::Class);
    assert_eq!(symbol(&results, "OrderStatus").kind, SymbolKind::Enum);
    assert_eq!(symbol(&results, "Pending").kind, SymbolKind::EnumMember);
    assert_eq!(symbol(&results, "Sku").kind, SymbolKind::Type);
    assert_eq!(symbol(&results, "Vehicle").kind, SymbolKind::Field);
    assert_eq!(symbol(&results, "Catalog").kind, SymbolKind::Field);
    assert_eq!(symbol(&results, "ItemKey").kind, SymbolKind::Constant);
    assert_eq!(
        fact_symbol(&results, "xml.xsd.type.v1", "type_name", "BaseEntity"),
        "BaseEntity"
    );
    assert_eq!(
        fact_symbol(&results, "xml.xsd.element.v1", "element_name", "Vehicle"),
        "Vehicle"
    );
}

#[test]
fn schema_qname_attributes_become_type_usage_identifiers() {
    let results = extract("orders.xsd", ORDERS_XSD);
    let usages = identifiers(&results, IdentifierKind::TypeUsage);

    for name in ["Sku", "Vehicle", "ItemKey", "Car", "BaseEntity"] {
        assert!(usages.contains(&name), "{name} in {usages:?}");
    }
    assert_eq!(
        usages
            .iter()
            .filter(|name| **name == "Sku" || **name == "OrderStatus")
            .count(),
        4
    );
}

#[test]
fn schema_references_resolve_to_same_file_components_and_pend_across_namespaces() {
    let results = extract("orders.xsd", ORDERS_XSD);

    assert_eq!(
        edges(&results, RelationshipKind::Extends),
        BTreeSet::from([("Order".to_string(), "BaseEntity".to_string())])
    );
    let references = edges(&results, RelationshipKind::References);
    for (from, to) in [
        ("shipTo", "Address"),
        ("status", "OrderStatus"),
        ("Skus", "Sku"),
        ("Code", "Sku"),
        ("Code", "OrderStatus"),
        ("Car", "Vehicle"),
        ("Catalog", "Car"),
        ("ItemRef", "ItemKey"),
    ] {
        assert!(
            references.contains(&(from.to_string(), to.to_string())),
            "{from} -> {to} in {references:?}"
        );
    }
    assert_eq!(
        pending(&results, RelationshipKind::References),
        vec![("Money".to_string(), Some("common.xsd".to_string()))]
    );
    let pending_row = results
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.target.terminal_name == "Money")
        .unwrap();
    assert_eq!(
        pending_row.target.namespace_path,
        vec!["urn:example:common"]
    );
    assert!(pending_row.reference_site_is_exact);
}

const WEATHER_WSDL: &str = r#"<?xml version="1.0"?>
<wsdl:definitions xmlns:wsdl="http://schemas.xmlsoap.org/wsdl/"
                  xmlns:soap="http://schemas.xmlsoap.org/wsdl/soap/"
                  xmlns:xs="http://www.w3.org/2001/XMLSchema"
                  xmlns:tns="urn:weather" targetNamespace="urn:weather">
  <wsdl:types>
    <xs:schema targetNamespace="urn:weather">
      <xs:import namespace="urn:common" schemaLocation="common.xsd"/>
      <xs:complexType name="Forecast"/>
      <xs:element name="GetForecast" type="tns:Forecast"/>
    </xs:schema>
  </wsdl:types>
  <wsdl:message name="GetForecastRequest">
    <wsdl:part name="body" element="tns:GetForecast"/>
  </wsdl:message>
  <wsdl:portType name="WeatherPortType">
    <wsdl:operation name="GetForecast">
      <wsdl:documentation>Returns the forecast for a city.</wsdl:documentation>
      <wsdl:input message="tns:GetForecastRequest"/>
    </wsdl:operation>
  </wsdl:portType>
  <wsdl:binding name="WeatherBinding" type="tns:WeatherPortType"/>
  <wsdl:service name="Weather">
    <wsdl:port name="WeatherPort" binding="tns:WeatherBinding">
      <soap:address location="https://api.example.com/weather"/>
    </wsdl:port>
  </wsdl:service>
</wsdl:definitions>
"#;

#[test]
fn wsdl_components_take_declared_kinds_and_resolve_references() {
    let results = extract("weather.wsdl", WEATHER_WSDL);

    assert_eq!(
        symbol(&results, "GetForecastRequest").kind,
        SymbolKind::Struct
    );
    assert_eq!(
        symbol(&results, "WeatherPortType").kind,
        SymbolKind::Interface
    );
    assert_eq!(symbol(&results, "WeatherBinding").kind, SymbolKind::Class);
    assert_eq!(symbol(&results, "WeatherPort").kind, SymbolKind::Property);
    assert_eq!(symbol(&results, "Weather").kind, SymbolKind::Module);
    let operation = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "GetForecast" && symbol.kind == SymbolKind::Method)
        .expect("GetForecast operation");
    assert_eq!(
        operation.doc_comment.as_deref(),
        Some("Returns the forecast for a city.")
    );
    let references = edges(&results, RelationshipKind::References);
    for (from, to) in [
        ("body", "GetForecast"),
        ("GetForecast", "GetForecastRequest"),
        ("WeatherBinding", "WeatherPortType"),
        ("WeatherPort", "WeatherBinding"),
        ("GetForecast", "Forecast"),
    ] {
        assert!(
            references.contains(&(from.to_string(), to.to_string())),
            "{from} -> {to} in {references:?}"
        );
    }
}

#[test]
fn wsdl_inline_schema_emits_schema_facts_and_ports_carry_their_address() {
    let results = extract("weather.wsdl", WEATHER_WSDL);

    assert_eq!(facts_with_pattern(&results, "xml.xsd.type.v1").len(), 1);
    assert_eq!(facts_with_pattern(&results, "xml.xsd.element.v1").len(), 1);
    assert_eq!(facts_with_pattern(&results, "xml.xsd.import.v1").len(), 1);
    let schema = facts_with_pattern(&results, "xml.xsd.schema.v1");
    assert_eq!(
        metadata_str(schema[0], "target_namespace"),
        Some("urn:weather")
    );
    let port = facts_with_pattern(&results, "xml.wsdl.port.v1");
    assert_eq!(
        metadata_str(port[0], "address_location"),
        Some("https://api.example.com/weather")
    );
    assert_eq!(
        fact_symbol(&results, "xml.wsdl.port.v1", "port_name", "WeatherPort"),
        "WeatherPort"
    );
    let document = facts_with_pattern(&results, "xml.document.v1");
    assert_eq!(
        metadata_str(document[0], "target_namespace"),
        Some("urn:weather")
    );
}

#[test]
fn xml_family_file_types_route_to_the_xml_extractor() {
    let document = "<root name=\"r\"><item name=\"a\"/></root>\n";
    for file_path in [
        "MainWindow.xaml",
        "App.axaml",
        "transform.xsl",
        "transform.xslt",
        "Native.vcxproj",
        "Build.proj",
        "Db.sqlproj",
        "test.runsettings",
        "strings.xlf",
        "strings.xliff",
        "Web.config",
        "app.exe.config",
    ] {
        let results = extract(file_path, document);
        assert!(
            results
                .symbols
                .iter()
                .any(|symbol| symbol.language == "xml"),
            "{file_path}"
        );
    }
    assert_eq!(
        crate::language_spec::detect_language_for_source("tool.config", "key = value\n"),
        None
    );
}

#[test]
fn android_manifest_components_are_facts_and_class_identifiers() {
    let source = r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.example.orders">
  <uses-permission android:name="android.permission.INTERNET"/>
  <permission android:name="com.example.orders.SYNC"/>
  <application android:name=".OrdersApp">
    <activity android:name=".ui.MainActivity" android:exported="true">
      <intent-filter>
        <action android:name="android.intent.action.MAIN"/>
        <category android:name="android.intent.category.LAUNCHER"/>
      </intent-filter>
    </activity>
    <service android:name="com.example.orders.SyncService"/>
  </application>
</manifest>
"#;
    let results = extract("app/src/main/AndroidManifest.xml", source);

    for name in [
        ".ui.MainActivity",
        "android.permission.INTERNET",
        "android.intent.action.MAIN",
        "com.example.orders.SyncService",
    ] {
        assert!(!has_symbol(&results, name), "{name} is not a declaration");
    }
    assert!(has_symbol(&results, "com.example.orders.SYNC"));
    let usages = identifiers(&results, IdentifierKind::TypeUsage);
    assert!(usages.contains(&"MainActivity"));
    assert!(usages.contains(&"SyncService"));
    assert!(usages.contains(&"OrdersApp"));
    let activity = facts_with_pattern(&results, "xml.android_component.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "component") == Some("activity"))
        .unwrap();
    assert_eq!(
        metadata_str(activity, "class"),
        Some("com.example.orders.ui.MainActivity")
    );
    let metadata = activity.metadata.as_ref().unwrap();
    assert_eq!(metadata["exported"], serde_json::Value::Bool(true));
    assert_eq!(
        metadata["intent_actions"],
        serde_json::json!(["android.intent.action.MAIN"])
    );
    let permissions: Vec<_> = facts_with_pattern(&results, "xml.android_permission.v1")
        .into_iter()
        .map(|fact| {
            (
                metadata_str(fact, "permission"),
                metadata_str(fact, "usage"),
            )
        })
        .collect();
    assert_eq!(
        permissions,
        vec![
            (Some("android.permission.INTERNET"), Some("uses")),
            (Some("com.example.orders.SYNC"), Some("declares")),
        ]
    );
}

#[test]
fn android_layout_ids_declare_fields_and_references_are_identifiers() {
    let source = r#"<LinearLayout xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:tools="http://schemas.android.com/tools" tools:context=".MainActivity">
  <TextView android:id="@+id/title" android:text="@string/title"/>
  <com.example.orders.ui.OrderListView android:id="@+id/orders" android:layout_below="@id/title"/>
  <Button android:id="@+id/save" android:onClick="onSave"/>
</LinearLayout>
"#;
    let results = extract("res/layout/activity_main.xml", source);

    let names: Vec<_> = results.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["title", "orders", "save"]);
    assert!(results.symbols.iter().all(|s| s.kind == SymbolKind::Field));
    assert_eq!(identifiers(&results, IdentifierKind::Call), vec!["onSave"]);
    assert_eq!(
        identifiers(&results, IdentifierKind::TypeUsage),
        vec!["MainActivity", "OrderListView"]
    );
    assert_eq!(
        identifiers(&results, IdentifierKind::VariableRef),
        vec!["title", "title"]
    );
}

#[test]
fn spring_beans_wire_references_identifiers_and_facts() {
    let source = r#"<beans xmlns="http://www.springframework.org/schema/beans"
       xmlns:context="http://www.springframework.org/schema/context">
  <context:component-scan base-package="com.example.orders"/>
  <bean id="orderService" class="com.example.orders.OrderService" init-method="start">
    <property name="repository" ref="orderRepository"/>
    <constructor-arg ref="dataSource"/>
  </bean>
  <bean id="orderRepository" class="com.example.orders.JdbcOrderRepository"/>
</beans>
"#;
    let results = extract("applicationContext.xml", source);

    assert_eq!(symbol(&results, "orderService").kind, SymbolKind::Variable);
    assert_eq!(symbol(&results, "repository").kind, SymbolKind::Property);
    assert_eq!(
        identifiers(&results, IdentifierKind::TypeUsage),
        vec!["OrderService", "JdbcOrderRepository"]
    );
    assert_eq!(identifiers(&results, IdentifierKind::Call), vec!["start"]);
    assert_eq!(
        edges(&results, RelationshipKind::References),
        BTreeSet::from([("repository".to_string(), "orderRepository".to_string())])
    );
    assert_eq!(
        pending(&results, RelationshipKind::References),
        vec![("dataSource".to_string(), None)]
    );
    assert_eq!(
        fact_symbol(&results, "xml.spring_bean.v1", "bean_id", "orderService"),
        "orderService"
    );
    let bean = facts_with_pattern(&results, "xml.spring_bean.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "bean_id") == Some("orderService"))
        .unwrap();
    assert_eq!(
        metadata_str(bean, "class"),
        Some("com.example.orders.OrderService")
    );
    assert_eq!(metadata_str(bean, "init_method"), Some("start"));
    let scan = facts_with_pattern(&results, "xml.spring_component_scan.v1");
    assert_eq!(
        metadata_str(scan[0], "base_package"),
        Some("com.example.orders")
    );
}

const MAPPER: &str = r#"<mapper namespace="com.example.users.UserMapper">
  <resultMap id="userResult" type="com.example.users.User"/>
  <sql id="columns">user_id, email</sql>
  <select id="findById" parameterType="long" resultMap="userResult">
    SELECT <include refid="columns"/> FROM users WHERE user_id = #{id}
  </select>
  <insert id="insertUser"><![CDATA[ INSERT INTO users (email) VALUES (#{email}) ]]></insert>
</mapper>
"#;

#[test]
fn mybatis_statements_are_methods_of_the_mapper_with_query_facts() {
    let results = extract("UserMapper.xml", MAPPER);
    let mapper = symbol(&results, "com.example.users.UserMapper");

    assert_eq!(mapper.kind, SymbolKind::Module);
    for name in ["findById", "insertUser"] {
        let statement = symbol(&results, name);
        assert_eq!(statement.kind, SymbolKind::Method);
        assert_eq!(statement.parent_id.as_deref(), Some(mapper.id.as_str()));
    }
    assert_eq!(symbol(&results, "userResult").kind, SymbolKind::Struct);
    assert_eq!(
        identifiers(&results, IdentifierKind::TypeUsage),
        vec!["UserMapper", "User"]
    );
    let references = edges(&results, RelationshipKind::References);
    assert!(references.contains(&("findById".to_string(), "userResult".to_string())));
    assert!(references.contains(&("findById".to_string(), "columns".to_string())));
    let statements = facts_with_pattern(&results, "xml.mybatis_statement.v1");
    let find = statements
        .iter()
        .find(|fact| metadata_str(fact, "statement_id") == Some("findById"))
        .unwrap();
    assert_eq!(metadata_str(find, "operation"), Some("select"));
    assert_eq!(
        metadata_str(find, "namespace"),
        Some("com.example.users.UserMapper")
    );
    assert_eq!(
        metadata_str(find, "sql"),
        Some("SELECT FROM users WHERE user_id = #{id}")
    );
    let insert = statements
        .iter()
        .find(|fact| metadata_str(fact, "statement_id") == Some("insertUser"))
        .unwrap();
    assert_eq!(
        metadata_str(insert, "sql"),
        Some("INSERT INTO users (email) VALUES (#{email})")
    );
    assert_eq!(
        fact_symbol(
            &results,
            "xml.mybatis_statement.v1",
            "statement_id",
            "findById"
        ),
        "findById"
    );
}

#[test]
fn mybatis_statement_bodies_are_embedded_sql_regions() {
    let results = extract("UserMapper.xml", MAPPER);
    let embedded: Vec<_> = results
        .source_regions
        .iter()
        .filter(|region| region.kind == SourceRegionKind::Embedded)
        .collect();

    assert_eq!(embedded.len(), 3);
    assert!(embedded.iter().all(|region| {
        region
            .metadata
            .as_ref()
            .and_then(|m| m.get("embedded_language"))
            == Some(&serde_json::Value::String("sql".to_string()))
    }));
}

#[test]
fn appsettings_keys_become_variables_and_config_facts() {
    let source = r#"<configuration>
  <appSettings>
    <add key="FeatureFlags:NewCheckout" value="true"/>
    <add key="Api:BaseUrl" value="https://api.example.com/v2"/>
  </appSettings>
  <connectionStrings>
    <add name="OrdersDb" connectionString="Server=.;Database=Orders"/>
  </connectionStrings>
</configuration>
"#;
    let results = extract("Web.config", source);
    let flag = symbol(&results, "FeatureFlags:NewCheckout");

    assert_eq!(flag.kind, SymbolKind::Variable);
    assert_eq!(
        flag.metadata.as_ref().unwrap()["value"],
        serde_json::Value::String("true".to_string())
    );
    assert!(has_symbol(&results, "OrdersDb"));
    let entries: Vec<_> = facts_with_pattern(&results, "xml.config_entry.v1")
        .into_iter()
        .map(|fact| {
            (
                metadata_str(fact, "key"),
                metadata_str(fact, "value"),
                metadata_str(fact, "section"),
            )
        })
        .collect();
    assert_eq!(
        entries,
        vec![
            (
                Some("FeatureFlags:NewCheckout"),
                Some("true"),
                Some("appSettings")
            ),
            (
                Some("Api:BaseUrl"),
                Some("https://api.example.com/v2"),
                Some("appSettings")
            ),
        ]
    );
    assert_eq!(
        fact_symbol(&results, "xml.config_entry.v1", "key", "Api:BaseUrl"),
        "Api:BaseUrl"
    );
}

#[test]
fn web_xml_mappings_become_route_facts_and_class_identifiers() {
    let source = r#"<web-app xmlns="https://jakarta.ee/xml/ns/jakartaee" version="6.0">
  <servlet><servlet-name>orders</servlet-name><servlet-class>com.example.web.OrdersServlet</servlet-class></servlet>
  <servlet-mapping><servlet-name>orders</servlet-name><url-pattern>/api/orders/*</url-pattern></servlet-mapping>
  <filter><filter-name>auth</filter-name><filter-class>com.example.web.AuthFilter</filter-class></filter>
  <filter-mapping><filter-name>auth</filter-name><url-pattern>/api/*</url-pattern></filter-mapping>
</web-app>
"#;
    let results = extract("WEB-INF/web.xml", source);

    assert!(has_symbol(&results, "orders"));
    assert!(has_symbol(&results, "auth"));
    assert_eq!(
        identifiers(&results, IdentifierKind::TypeUsage),
        vec!["OrdersServlet", "AuthFilter"]
    );
    let routes: Vec<_> = facts_with_pattern(&results, "xml.servlet_route.v1")
        .into_iter()
        .map(|fact| {
            (
                metadata_str(fact, "mapping_kind"),
                metadata_str(fact, "route_template"),
                metadata_str(fact, "target_name"),
                metadata_str(fact, "target_class"),
            )
        })
        .collect();
    assert_eq!(
        routes,
        vec![
            (
                Some("servlet"),
                Some("/api/orders/*"),
                Some("orders"),
                Some("com.example.web.OrdersServlet")
            ),
            (
                Some("filter"),
                Some("/api/*"),
                Some("auth"),
                Some("com.example.web.AuthFilter")
            ),
        ]
    );
}

const ANT_BUILD: &str = r#"<project name="orders" default="dist">
  <import file="common-build.xml"/>
  <property name="src.dir" value="src"/>
  <target name="dist" depends="compile, test" description="Build the jar">
    <antcall target="package"/>
  </target>
  <target name="compile"><javac srcdir="${src.dir}"/></target>
  <target name="test" depends="compile"/>
  <target name="package" depends="clean"/>
</project>
"#;

#[test]
fn ant_targets_are_functions_that_call_their_dependencies() {
    let results = extract("build.xml", ANT_BUILD);

    for name in ["dist", "compile", "test", "package"] {
        assert_eq!(symbol(&results, name).kind, SymbolKind::Function, "{name}");
    }
    assert_eq!(symbol(&results, "orders").kind, SymbolKind::Module);
    assert_eq!(
        symbol(&results, "dist").doc_comment.as_deref(),
        Some("Build the jar")
    );
    assert_eq!(
        edges(&results, RelationshipKind::Calls),
        BTreeSet::from([
            ("orders".to_string(), "dist".to_string()),
            ("dist".to_string(), "compile".to_string()),
            ("dist".to_string(), "test".to_string()),
            ("dist".to_string(), "package".to_string()),
            ("test".to_string(), "compile".to_string()),
        ])
    );
    assert_eq!(
        pending(&results, RelationshipKind::Calls),
        vec![("clean".to_string(), None)]
    );
    assert_eq!(
        pending(&results, RelationshipKind::Imports),
        vec![(
            "common-build.xml".to_string(),
            Some("common-build.xml".to_string())
        )]
    );
    assert_eq!(
        identifiers(&results, IdentifierKind::VariableRef),
        vec!["src.dir"]
    );
}

#[test]
fn msbuild_hook_targets_carry_their_usage_and_external_targets_pend() {
    let source = r#"<Project>
  <Target Name="GenerateDocs" AfterTargets="Build" DependsOnTargets="Compile;ResolveReferences">
    <CallTarget Targets="PublishDocs"/>
  </Target>
  <Target Name="PublishDocs"/>
</Project>
"#;
    let results = extract("Docs.targets", source);
    let call = results
        .relationships
        .iter()
        .find(|relationship| relationship.kind == RelationshipKind::Calls)
        .unwrap();

    assert_eq!(name_of(&results, &call.to_symbol_id), "PublishDocs");
    assert_eq!(
        call.metadata.as_ref().unwrap()["target_usage"],
        serde_json::Value::String("call".to_string())
    );
    let pending_calls: BTreeSet<_> = pending(&results, RelationshipKind::Calls)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        pending_calls,
        BTreeSet::from([
            "Build".to_string(),
            "Compile".to_string(),
            "ResolveReferences".to_string()
        ])
    );
}

#[test]
fn document_links_become_facts_and_pending_imports() {
    let source = r#"<?xml version="1.0"?>
<?xml-stylesheet type="text/xsl" href="book.xsl"?>
<book name="guide" xmlns:xi="http://www.w3.org/2001/XInclude"
      xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
      xsi:schemaLocation="urn:book book.xsd">
  <xi:include href="chapter2.xml"/>
</book>
"#;
    let results = extract("book.xml", source);
    let links: Vec<_> = facts_with_pattern(&results, "xml.document_link.v1")
        .into_iter()
        .map(|fact| (metadata_str(fact, "link_kind"), metadata_str(fact, "href")))
        .collect();

    assert_eq!(
        links,
        vec![
            (Some("stylesheet"), Some("book.xsl")),
            (Some("schema_location"), Some("book.xsd")),
            (Some("xinclude"), Some("chapter2.xml")),
        ]
    );
    let imports: BTreeSet<_> = pending(&results, RelationshipKind::Imports)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        imports,
        BTreeSet::from(["book.xsd".to_string(), "chapter2.xml".to_string()])
    );
}

#[test]
fn dtd_declarations_become_symbols_and_entity_references_resolve() {
    let source = r#"<!DOCTYPE book [
  <!ELEMENT book (title, chapter+)>
  <!ENTITY publisher "Contoso Press">
  <!ENTITY legal SYSTEM "legal.xml">
]>
<book name="guide"><title>Guide by &publisher; &amp; friends</title><script><![CDATA[ if (a < b) run(); ]]></script>&legal;</book>
"#;
    let results = extract("book.xml", source);

    assert_eq!(symbol(&results, "book").kind, SymbolKind::Type);
    let publisher = symbol(&results, "publisher");
    assert_eq!(publisher.kind, SymbolKind::Constant);
    assert_eq!(
        publisher.metadata.as_ref().unwrap()["value"],
        serde_json::Value::String("Contoso Press".to_string())
    );
    let references: Vec<_> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::VariableRef)
        .map(|identifier| {
            (
                identifier.name.as_str(),
                identifier
                    .target_symbol_id
                    .as_deref()
                    .map(|id| name_of(&results, id)),
            )
        })
        .collect();
    assert_eq!(
        references,
        vec![("publisher", Some("publisher")), ("legal", Some("legal"))]
    );
    let links: Vec<_> = facts_with_pattern(&results, "xml.document_link.v1")
        .into_iter()
        .map(|fact| metadata_str(fact, "link_kind"))
        .collect();
    assert_eq!(links, vec![Some("external_entity")]);
    assert!(results.source_regions.iter().any(|region| {
        region.kind == SourceRegionKind::StringLiteral
            && &source[region.start_byte as usize..region.end_byte as usize]
                == " if (a < b) run(); "
    }));
}

#[test]
fn resx_skips_the_schema_header_and_keeps_data_entries_as_constants() {
    let source = r#"<root>
  <xsd:schema id="root" xmlns="" xmlns:xsd="http://www.w3.org/2001/XMLSchema">
    <xsd:element name="root"><xsd:complexType><xsd:attribute name="name" type="xsd:string"/></xsd:complexType></xsd:element>
  </xsd:schema>
  <resheader name="resmimetype"><value>text/microsoft-resx</value></resheader>
  <data name="Spreadsheet_FileTab" xml:space="preserve">
    <value>File</value>
    <comment>Label of the first ribbon tab.</comment>
  </data>
</root>
"#;
    let results = extract("Strings.resx", source);

    let names: Vec<_> = results.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Spreadsheet_FileTab"]);
    let data = symbol(&results, "Spreadsheet_FileTab");
    assert_eq!(data.kind, SymbolKind::Constant);
    assert_eq!(
        data.metadata.as_ref().unwrap()["value"],
        serde_json::Value::String("File".to_string())
    );
    assert_eq!(
        data.doc_comment.as_deref(),
        Some("Label of the first ribbon tab.")
    );
}

#[test]
fn testng_suites_are_test_containers_and_classes_are_identifiers() {
    let source = r#"<suite name="RegressionSuite">
  <test name="CheckoutTests">
    <classes>
      <class name="com.example.checkout.CartTest">
        <methods><include name="addsItemToCart"/><exclude name="flakyPayment"/></methods>
      </class>
    </classes>
  </test>
</suite>
"#;
    let results = extract("testng.xml", source);

    let names: Vec<_> = results.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["RegressionSuite", "CheckoutTests"]);
    for symbol in &results.symbols {
        assert_eq!(
            symbol.metadata.as_ref().unwrap().get("test_container"),
            Some(&serde_json::Value::Bool(true)),
            "{}",
            symbol.name
        );
    }
    assert_eq!(
        identifiers(&results, IdentifierKind::TypeUsage),
        vec!["CartTest"]
    );
    assert_eq!(
        identifiers(&results, IdentifierKind::Call),
        vec!["addsItemToCart", "flakyPayment"]
    );
    let selection = facts_with_pattern(&results, "xml.test_selection.v1");
    let metadata = selection[0].metadata.as_ref().unwrap();
    assert_eq!(metadata["class"], "com.example.checkout.CartTest");
    assert_eq!(metadata["test"], "CheckoutTests");
    assert_eq!(metadata["suite"], "RegressionSuite");
    assert_eq!(
        metadata["included_methods"],
        serde_json::json!(["addsItemToCart"])
    );
    assert_eq!(
        metadata["excluded_methods"],
        serde_json::json!(["flakyPayment"])
    );
}

#[test]
fn xslt_named_templates_are_functions_that_call_template_calls() {
    let source = r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:import href="common.xsl"/>
  <xsl:template match="/">
    <xsl:call-template name="header"><xsl:with-param name="title" select="'Orders'"/></xsl:call-template>
    <xsl:element name="footer"/>
  </xsl:template>
  <xsl:template name="header"><xsl:param name="title"/></xsl:template>
</xsl:stylesheet>
"#;
    let results = extract("orders.xslt", source);

    let names: Vec<_> = results.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["header", "title"]);
    assert_eq!(symbol(&results, "header").kind, SymbolKind::Function);
    assert_eq!(identifiers(&results, IdentifierKind::Call), vec!["header"]);
    let call = &results.identifiers[0];
    assert_eq!(
        call.target_symbol_id
            .as_deref()
            .map(|id| name_of(&results, id)),
        Some("header")
    );
    let links: Vec<_> = facts_with_pattern(&results, "xml.document_link.v1")
        .into_iter()
        .map(|fact| metadata_str(fact, "link_kind"))
        .collect();
    assert_eq!(links, vec![Some("xsl_import")]);
}

#[test]
fn xaml_class_and_named_elements_are_declarations() {
    let source = r#"<Window x:Class="Orders.Views.MainWindow"
        xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
        xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Grid><Button x:Name="SaveButton" Content="Save"/><TextBox Name="Query"/></Grid>
</Window>
"#;
    let results = extract("MainWindow.xaml", source);

    let window = symbol(&results, "MainWindow");
    assert_eq!(window.kind, SymbolKind::Class);
    assert_eq!(
        window.metadata.as_ref().unwrap()["qualified_name"],
        serde_json::Value::String("Orders.Views.MainWindow".to_string())
    );
    assert_eq!(symbol(&results, "SaveButton").kind, SymbolKind::Field);
    assert_eq!(symbol(&results, "Query").kind, SymbolKind::Field);
}
