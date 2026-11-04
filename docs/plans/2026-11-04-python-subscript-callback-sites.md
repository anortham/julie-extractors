# Python subscript callback sites

## Evidence

The v3.12.0 downstream round found `environ["werkzeug.debug.preserve_context"](ctx)`
in Flask `d73fa1c`, `src/flask/app.py:1610`. Python identifier extraction handles
named and attribute callees, then skips other function-node forms. The omission
also exists in the 3.11.0 source. This is an open extraction coverage gap, not
an incorrect named-callee row or a claim of complete call-graph recovery.

## Closure task: preserve unresolved subscript invocation sites

- Specify a source-local representation for a call whose function node is a
  subscript. Preserve the written callee and its span; do not turn a literal
  dictionary key into a proven function name or resolve the callback target.
- Implement it in `crates/julie-extractors/src/python/identifiers.rs` with
  registered golden evidence and contract review for any added metadata shape.
- Cover literal and computed keys, nested callable expressions, Unicode/CRLF
  offsets, and argument facts. A subscript read without invocation is a
  negative control; named and attribute calls must retain their current rows.
- Verify the exact Flask site and the consumer's callees output. It must expose
  the unresolved source site without inventing a target. Run the narrow Python,
  capability, golden, compatibility and affected corpus checks.

This is a named follow-up outside v3.12.0's implemented construct set. The
release policy allows honest coverage gaps; the release notes and structured
downstream `open_gaps` record preserve the limitation without claiming closure.
