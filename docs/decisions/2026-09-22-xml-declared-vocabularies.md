# XML symbols by declared vocabulary

Date: 2026-09-22. Plan: [language gap closure](../plans/2026-09-22-language-gap-closure.md),
wave 2 (the `xml.*` gaps).

## Decision

- The symbol kind comes from what the element declares when its vocabulary is
  known. The vocabulary comes from the namespace URI that the element prefix
  resolves to through the in-scope `xmlns` declarations (XML Schema, WSDL,
  XSLT, XAML), or from the document (MSBuild, Ant, Spring, MyBatis, TestNG,
  Android, `.resx`). Examples:
  - `xs:complexType` is a class and `xs:element` is a field.
  - A WSDL operation is a method and a WSDL message is a struct.
  - An Ant or MSBuild target and an XSLT named template are functions.
- A document in an unknown vocabulary keeps the structural rule. An element
  with child elements is a module, and a leaf element is a variable.
- An element whose name attribute refers to something else declares nothing.
  These elements are Android components and platform constants, TestNG
  classes and method selections, XSLT `call-template` and `with-param`, and
  the `.resx` schema header. Their names become identifiers.
- QName attributes resolve through the in-scope `xmlns` declarations. A
  component in the same file under the matching `targetNamespace` gets an
  edge. A component in any other namespace gets a structured pending row. The
  row carries the namespace URI and the `schemaLocation` of the matching
  `import`.
- A structural fact that anchors on a declaration element binds to that
  declaration's symbol, of any kind. Other facts keep the shared
  containing-symbol rule.
- A `.config` file routes to `xml` when its first non-blank text (after a byte
  order mark) starts with `<`. Discovery reads at most the first 256 bytes of a
  `.config` file to decide. Other `.config` files stay unsupported.

## Why

Before this change, one construct took two kinds: a `<target>` with children
was a module, and a `<target/>` without children was a variable. Facts on
variable-kind declarations lost their owner. Class names in Spring, Android,
servlet, TestNG, and MyBatis files became false declarations. Schema
references were identifiers only, so no edges came from them.

## Windows

Discovery opens a `.config` file read-only and drops the handle before it
returns. Path output is unchanged.
