# SQL support

`sql` handles `.sql` files with the `tree-sitter-sequel` grammar. One grammar
covers the PostgreSQL, T-SQL, MySQL, and SQLite dialects.

## Symbols

- A table is a `class` symbol, a view or CTE an `interface` symbol, a
  procedure or function a `function` symbol, and a trigger a `method` symbol.
- `ALTER PROCEDURE` and `ALTER FUNCTION` give the same `function` symbol as
  their `CREATE` form. The signature starts with `ALTER`.
- A schema-qualified declaration (`CREATE TABLE billing.accounts`) records the
  schema in the `schema` metadata key.
- A T-SQL procedure with parameters outside parentheses
  (`CREATE PROCEDURE dbo.p @Id INT, @Name NVARCHAR(50) AS ...`) keeps its real
  name and gets one parameter symbol for each `@` parameter.
- `ALTER TABLE ... ADD CONSTRAINT` gives an `interface` constraint symbol and
  `ALTER TABLE ... ADD [COLUMN]` a `field` symbol. Both are children of the
  table when the table is declared in the same file. Otherwise they carry the
  `table` (and `schema`) metadata keys.

## Body spans

- A procedure or function body is its `function_body` node: `AS BEGIN ... END`
  in T-SQL, `AS $$ ... $$` in PostgreSQL. When the grammar splits a body, the
  body comes from the statement text and the metadata key `bodySpanSource`
  says `statement_text`.
- Columns and select aliases have no body.

## References and calls

Each table or view name inside a routine, view, trigger, or `CREATE TABLE AS`
is a reference from that object:

- `FROM`, `JOIN`, `UPDATE`, `DELETE FROM`, `INSERT INTO`, `MERGE`, and
  `TRUNCATE` targets, and a trigger's `ON` target.
- The name is a `type_usage` identifier.
- The edge is a `references` relationship when the table or view is declared
  in the same file. Its `relationshipType` metadata is `view_source`,
  `trigger_target`, or `table_reference`.
- Otherwise the edge is a structured pending `references` relationship. The
  schema goes in `namespace_path`.

Each function invocation, `EXEC`/`EXECUTE` target, and trigger
`EXECUTE FUNCTION` target is a `calls` edge with the same rules. The `EXEC`
target is a `call` identifier.

A name matches a same-file declaration only when the schemas agree. An
unqualified name on either side matches any schema, and a name with two
matches stays pending. So `REFERENCES identity.Users` never binds to a local
`dbo.Users`.

A CTE name in the same object, and the T-SQL `inserted` and `deleted` rows
inside a trigger, are not table references.
