# Rust Documentation Examples

An executable Rust example:

```rust
fn explicit_example() {}
```

This example only compiles:

```rust,no_run
fn no_run_example() {}
```

This example documents a compilation error:

```rust,compile_fail
let value: i32 = "not an integer";
```

An unspecified fence is Rust by default:

```
fn default_example() {}
```

Ignored examples are not doctests:

```rust,ignore
fn ignored_example() {}
```

Other languages are documentation snippets only:

```python
print("not a Rust doctest")
```
