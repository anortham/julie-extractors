# PowerShell support

`powershell` handles `.ps1`, `.psm1`, and `.psd1` files.

## Continuous testing

Run the language target when changing PowerShell extraction:

```bash
cargo xtask test language powershell
```

The command runs `tests::powershell::` and the golden extraction test with
`JULIE_GOLDEN_LANGUAGE=powershell`.

## Type facts

- Parameter types (`[string]$Name`), typed assignment targets
  (`[Foo]$x = ...`), class property types, class method return types, and a
  function's `[OutputType([T])]` are declared type facts. `[void]` records
  nothing.
- A variable assigned with no written type gets an inferred type fact from its
  value: a cast (`[Foo]$y`), `[Foo]::new(..)` or `New-Object Foo` for a
  same-file class `Foo`, or a call with a declared return type to one of these
  same-file callees:
  - a function (`Get-Foo`, `& Get-Foo`) whose `[OutputType(...)]` attributes
    name exactly one type literal;
  - an instance method of the enclosing class (`$this.Load()`);
  - a static method of a same-file class (`[Foo]::Create()`).
- Names match case-insensitively. Same-named candidates (overloads, repeated
  functions) must agree on the type. `$this.M()` looks only at instance
  methods and `[Foo]::M()` only at static methods.
- Parentheses around the value are looked through. A pipeline
  (`Get-Foo | Select-Object`), a longer chain (`$this.Load().Clone()`), a
  method inherited from a base class, an `OutputType` given as a string, two
  different output types, or a callee in another file records no fact. Other
  files are out of scope because each file is extracted alone.
- A written type always wins over inference.
