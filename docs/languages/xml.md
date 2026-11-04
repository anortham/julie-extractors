# XML

XML has no native program-variable access sites. MSBuild `$(Name)` and Ant `${name}` property substitutions are explicit reads and carry `metadata_json.access = "read"`. Schema QNames, resource names, MyBatis mapper references, and other XML-domain links remain without access metadata. Embedded SQL in mapper content is extracted using SQL syntax.

The generic symbol-annotation channel describes language decorators and
attributes. XSD `annotation/documentation` content uses `doc_comment`; XML
attributes and recognized vocabulary metadata keep their native symbols and
structural facts. These channels are classified separately in the coverage
matrix.

At full extraction level, XSLT produces file and template/function complexity
metrics. `if` and `when` count as decisions; `for-each`, `for-each-group` and
`iterate` count as loops. Nesting counts these controls, and callable parameter
counts include only direct `param` children. Namespace aliases are recognized
by URI; foreign elements with the same local names do not contribute. Plain
XML has no control-flow metric rows.
