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
    name exactly one type literal and whose body outputs one value (see
    below);
  - an instance method of the enclosing class or its base classes
    (`$this.Load()`);
  - a static method of a same-file class or its base classes
    (`[Foo]::Create()`).
- Names match case-insensitively. Same-named candidates (overloads, repeated
  functions) must agree on the type. `$this.M()` looks only at instance
  methods and `[Foo]::M()` only at static methods.
- A method call looks at every same-named method of the class and of its base
  classes, because PowerShell overload resolution sees inherited methods.
  `$this.M()` also looks at every same-file subclass, because `$this` can be
  a subclass instance that overrides `M`. The class and all its base classes
  must be in the file. A class with an external base (`class Foo : Bar` with
  no `Bar` in the file, or a .NET type or interface) records nothing, because
  the external base can add same-named methods.
- `[OutputType([T])]` names the type of each output item, not the call
  result. PowerShell sends every output statement to the caller and unrolls
  enumerable values, so a function call records a type only when:
  - `T` is not an array or a generic type, and is a listed .NET type that
    PowerShell does not unroll (string, the number types, `bool`, `char`,
    `datetime`, `timespan`, `guid`, `version`, `uri`, `hashtable`,
    `pscustomobject`, `psobject`, `scriptblock`, `regex`, `securestring`,
    `pscredential`, `FileInfo`, `DirectoryInfo`, `xml`), a same-file enum,
    or a same-file class whose base classes are all in the file. Any other
    external type records nothing, because it can be enumerable
    (`HashSet`, `DataView`, `StringDictionary`, a type from another file);
  - the body has at least one output, and either exactly one output
    statement or only `return <value>` outputs. An output statement other
    than `return` in a loop (`foreach`, `for`, `while`, `do`, `switch`)
    records nothing. Assignments, `[void]` casts, and pipelines that end in
    `Out-Null`, `Write-Verbose`, `Write-Debug`, `Write-Warning`,
    `Write-Host`, `Write-Information`, `Write-Error`, or `Write-Progress` are
    not outputs. Any other command or bare expression is an output;
  - each output value is one object built in place: `[T]::new(..)`,
    `New-Object`, a cast, a hashtable literal, a string or number literal, or
    `$this`. A variable, a member, a method call, a command, a subexpression,
    or an array (`return $items`, `return Get-Thing`, `return @(..)`,
    `return ,$x`) records nothing, because it can hold or emit a
    collection.
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
  - a function whose output type or body fails the rules above. PowerShell
    unrolls function output into the pipeline, so the variable holds one
    item, an `object[]`, or `$null`. Method calls do not unroll, so
    `[string[]] M()` still records `string[]`;
  - a static call or constructor on a generic type literal
    (`[Foo[int]]::Create()`), because PowerShell classes cannot be generic, so
    the name is never the same-file class;
  - a function name that a `Set-Alias`, `New-Alias`, `sal`, `nal`, or a
    function's `[Alias(...)]` attribute in the file also defines, because an
    alias wins over a function. An alias whose name is not a literal
    (`Set-Alias -Name $n`, `Set-Alias @args`) stops function inference in the
    whole file. An alias defined outside the file is not visible;
  - a call to a nested function from outside the function or class method
    that defines it, because the nested function exists only in that scope.
    A function in a script block counts as top level, because Pester blocks
    and dot-sourced blocks define it in the caller's scope. A function nested
    in a function with a parse error also counts as top level, because error
    recovery often nests the following top-level functions there;
  - an `OutputType` given as a string,
    two different output types, or a callee in another file. Other files are
    out of scope because each file is extracted alone.
- PowerShell does not check `[OutputType(...)]` at runtime. A function whose
  `OutputType` does not match what it outputs gives a wrong inferred fact.
  The extractor checks the shape of the body (how many outputs, and how each
  value is built) but not the type of each output value.
- A written type always wins over inference.
