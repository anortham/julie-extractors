# 0007: cURL Client Facts Emit on Configuration, Not Execution

## Context

The PHP cURL collector emits `http.client_request.v1` facts for handles whose
URL (and optionally verb) are statically configured via `curl_init` /
`curl_setopt`. A Codex review pass proposed requiring a `curl_exec($handle)`
call before emission, arguing that a configured-but-unexecuted handle is not an
outbound request.

## Decision

Emission stays configuration-based: a handle with a statically-known URL emits
in the scope where it is configured, whether or not `curl_exec` appears in that
scope.

## Rationale

- Consistency: the OkHttp collector emits on `Request.Builder().url(x).build()`
  and the hyper collector on `Request::builder().uri(x)` chains — both emit the
  constructed request without requiring an execute/send call. Requiring
  `curl_exec` would make cURL the only collector with an execution gate.
- Recall: the wrapper idiom (a helper configures and returns the handle, the
  caller executes it) is common in real PHP; per-scope tracking already
  prevents cross-scope joins, so an execution gate would silence the whole
  idiom.
- The false-positive risk (a fully configured handle that is deliberately never
  executed anywhere) is a degenerate case; the M2 receiver/static-argument
  proofs are the load-bearing precision controls.

The `kotlin/ktor_routes` and `php/http_client_deferred` goldens plus
`php_curl_configured_handle_emits_without_exec` lock this contract.

## Consequences

- Dead configured handles emit a fact. Consumers joining boundary edges should
  treat client facts as "request constructed here", not "request sent here" —
  matching the OkHttp/hyper semantics already documented in
  `docs/contracts/sqlite-schema-v4.md`.
- Options applied after `curl_exec` in the same scope still fold into the
  emitted fact; execution ordering within a scope is not modeled.
