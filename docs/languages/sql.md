# SQL support

`sql` handles `.sql` files with the `tree-sitter-sequel` grammar. One grammar
covers the PostgreSQL, T-SQL, MySQL, and SQLite dialects.

## Symbols

- A table is a `class` symbol, a view or CTE an `interface` symbol, a
  procedure or function a `function` symbol, and a trigger a `method` symbol.
- A materialized view is a view with `isMaterialized: true` and the signature
  `CREATE MATERIALIZED VIEW name`.
- An index is a `property` symbol. Its signature comes from the grammar:
  `CREATE [UNIQUE] INDEX name ON schema.table [USING method] (columns)
  [INCLUDE (columns)] [WHERE predicate]`. The metadata keys are `isUnique`
  and `table`.
- A constraint symbol exists only for a named constraint
  (`CONSTRAINT fk_team FOREIGN KEY ...`). An unnamed constraint is a
  structural fact only.
- A select alias (`sum(total) AS revenue`) is a `field` symbol only inside a
  view, a materialized view, or a CTE. The alias of a standalone query or a
  routine query is a result label, not a declaration, so it is not a symbol.
- A function has `isStoredProcedure: false`; a procedure has `true`. A
  function signature includes its `RETURNS` clause.
- Each PL/pgSQL `DECLARE` entry and each T-SQL or MySQL `DECLARE @x TYPE`
  entry is one local `variable` symbol with the signature `DECLARE name TYPE`.
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

## Doc comments

- A declaration takes the `--` or `/* */` comment block directly above it, or
  above the statement that holds it.
- A table or function comment never documents its columns or parameters.
- A comment after a column on the same line (`email TEXT, -- Login address.`)
  documents that column, not the next one.
- `COMMENT ON TABLE | VIEW | FUNCTION | COLUMN ... IS '...'` and the MySQL
  `COMMENT '...'` column attribute and `COMMENT='...'` table option set the
  doc comment of the named object. This catalog text replaces a source
  comment.

## Types

- A column, parameter, or local variable records its declared type from the
  grammar's type node. The resolved type is the base name (`NVARCHAR`,
  `NUMERIC`, `TEXT`). The full text (`NVARCHAR(50)`, `NUMERIC(10,2)`,
  `TEXT[]`) goes in the `declared` metadata key when it differs.
- A function records its `RETURNS` type: `SETOF orders` resolves to `orders`,
  and `TABLE(id INT)` resolves to `TABLE`.
- A table records `TABLE`, a view `VIEW`, and a procedure `PROCEDURE`.

## Body spans

- A procedure or function body is its `function_body` node: `AS BEGIN ... END`
  in T-SQL, `AS $$ ... $$` in PostgreSQL. When the grammar splits a body, the
  body comes from the statement text and the metadata key `bodySpanSource`
  says `statement_text`.
- Columns, select aliases, parameters, and local variables have no body.

## References and calls

Each table or view name inside a routine, view, trigger, or `CREATE TABLE AS`
is a reference from that object:

- `FROM`, `JOIN`, `UPDATE`, `DELETE FROM`, `INSERT INTO`, `MERGE`, and
  `TRUNCATE` targets, and a trigger's `ON` target.
- The name is a `type_usage` identifier.
- An index's `ON` table is also a reference from the index.
- The edge is a `references` relationship when the table or view is declared
  in the same file. Its `relationshipType` metadata is `view_source`,
  `trigger_target`, `index_table`, or `table_reference`.
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

A T-SQL `CREATE TRIGGER dbo.t ON dbo.Orders AFTER INSERT` does not parse. The
recovery names the trigger `t`, links it to `Orders` with a `trigger_target`
edge, and fills the trigger fact from the text.

## Identifiers

- A T-SQL `@variable` or `@parameter` use is a `variable_ref` identifier. The
  argument name in `EXEC proc @Name = value` belongs to the callee, so it is
  not an identifier.
- An unqualified name inside a routine that matches one of the routine's
  parameters or `DECLARE` variables is a `variable_ref` identifier.
- A column reference is a `member_access` identifier. Its `receiver_type` is
  the table the column reads: the table behind the alias (`o.total` with
  `FROM orders o`), the named table (`orders.total`), or the only table of the
  query when the column has no qualifier. A query that reads more than one
  table, a derived table, or a table variable leaves `receiver_type` empty.
- The target column list of `INSERT INTO t (a, b)` and of a `MERGE ... INSERT
  (a, b)` gives `member_access` identifiers with the target table as
  `receiver_type`.

## Literals

- Every quoted string is a literal, including `N'...'` and `E'...'` strings.
  Doubled quotes decode to one quote.
- The string in `EXEC(N'...')` has the carrier `EXEC`, and a string argument
  of `EXEC sp_executesql` has the carrier `sp_executesql`. `languages/sql.toml`
  classifies both as `sql` literals. Other literals stay `other`.

## Test roles

- pgTAP: `runtests(...)` or `do_tap(...)` marks functions that return
  `SETOF TEXT` as tests and fixtures, and marks the named schema as a test
  container. The documented `runtests('tests'::name)` form also works.
- tSQLt: `EXEC tSQLt.NewTestClass 'OrderTests'` makes `OrderTests` a test
  class. Its procedures whose names start with `test` are test cases, and its
  `SetUp` procedure is fixture setup. A declared schema of that name is a test
  container.

## Structural facts

- `sql.view_definition.v1` covers views and materialized views. The
  `materialized` key tells them apart. `source_tables` holds only the tables
  the query reads, never aliases, function names, or cast types.
- `sql.trigger_definition.v1` has `timing` (`before`, `after`, or
  `instead_of`), the first `event`, every event in `events`, and the executed
  `function_name`.
- `sql.function_definition.v1` and `sql.procedure_definition.v1` count the
  declared parameters in `parameter_count`.
- `sql.merge_statement.v1` covers a `MERGE` with a `VALUES`, table, or query
  source (`source_kind` `values`, `table`, or `query`).
- `sql.policy_definition.v1`, `sql.extension.v1`,
  `sql.sequence_definition.v1`, `sql.type_definition.v1`, and
  `sql.role_definition.v1` record PostgreSQL DDL objects.

## Complexity

- Decisions: each `WHEN` of a `CASE`, each `AND` and `OR`, `IF`, `WHERE`,
  `HAVING`, a join, a set operation, and each `MERGE` `WHEN` clause.
- Loops: `WHILE`.
- Views get a symbol metric. A routine metric has `parameter_count`.
