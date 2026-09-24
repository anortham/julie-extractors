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
- Parentheses around the value are looked through. These record no fact:
  - a pipeline (`Get-Foo | Select-Object`) or a longer chain
    (`$this.Load().Clone()`);
  - an operator or a leading comma around the value (`-not (Get-Foo)`,
    `-join (Get-Foo)`, `,(Get-Foo)`, `-not [Foo]$y`), because it changes the
    type;
  - a compound assignment (`$all += Get-Foo`, `$x ??= Get-Foo`), because it
    combines the value with the current one;
  - a script path call (`& ./lib/Get-Foo`), because it runs a file, not the
    same-file function;
  - a command with a redirection (`Get-Foo > $null`, `Get-Foo 2>&1`);
  - `$this.M()` inside a script block, because an `Add-Member` script method
    or an event action binds `$this` to another object;
  - a method inherited from a base class, an `OutputType` given as a string,
    two different output types, or a callee in another file. Other files are
    out of scope because each file is extracted alone.
- A written type always wins over inference.
