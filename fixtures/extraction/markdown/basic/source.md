---
title: Worker Guide
tags:
  - docs
  - api
---

# Worker Guide

Use `run_worker` to process a worker id.

## Usage

Review the [Worker API](https://api.example.com/workers) before running a job.

```rust
fn helper(value: i32) -> i32 {
    value + 1
}
```

[worker-ref]: https://api.example.com/workers "Worker API"

| Field | Value |
| ----- | ----- |
| id | 1 |
| name | fixture |

Operations
----------

See [the [v2] runbook](https://ops.example.com/runbook), jump to [usage](#usage),
or read the [worker reference][worker-ref] and the note[^ops].

![Pipeline diagram](docs/pipeline.png) Contact <ops@example.com>.

Regex classes such as `[^a-z]` and calls like `handlers[0](event)` are code.

```js
return handlers[name](req);
```

[^ops]: Operations run nightly.
