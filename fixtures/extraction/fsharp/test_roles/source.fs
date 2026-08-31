module DomainTests

open Xunit

[<Fact>]
let adds_numbers () = 1 + 1

[<Theory>]
let adds_numbers_from_data (value: int) = value + 1

[<Xunit.Fact>]
let qualified_fact () = true

let helper value = value
