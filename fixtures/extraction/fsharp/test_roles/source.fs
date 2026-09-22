module DomainTests

open Xunit

[<Fact>]
let adds_numbers () = 1 + 1

[<Theory>]
let adds_numbers_from_data (value: int) = value + 1

[<Xunit.Fact>]
let qualified_fact () = true

let helper value = value

[<NUnit.Framework.TestFixture>]
type OrderTests() =
    [<SetUp>]
    member _.Init() = ()

    [<Test>]
    member _.Starts() = Assert.Pass()

    [<TestCase(1, 2)>]
    member _.Increments(input: int, expected: int) = Assert.That(input + 1, Is.EqualTo(expected))

    [<TearDown>]
    member _.Cleanup() = ()

    member _.NotATest() = ()

[<TestClass>]
type CalculatorTests() =
    [<TestMethod>]
    member _.AddsNumbers() = ()

    [<DataTestMethod>]
    member _.AddsRows() = ()

[<Tests>]
let pricingTests = testList "pricing" []

let notATestValue = 1
