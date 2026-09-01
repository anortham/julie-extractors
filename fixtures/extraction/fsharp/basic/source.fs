module Domain =
  open System
  open System.Collections.Generic

  /// Coordinates used by the domain model.
  [<Struct>]
  type Point = { X: int; Y: int }

  type Shape =
    | Circle of radius: float
    | Empty

  type Id = int
  type Foo() = class end
  type Bar() = class end


  type Base() = class end

  type Calculator(value: int) =
    inherit Base()

    /// Current calculator value.
    [<Obsolete>]
    member _.Value = value

    member _.Calculate() =
      if value > 0 then value else 0

    static member Create() = Calculator(0)

    member this.Helper() = 0
    member this.Run(a: Bar) = this.Helper()
    member x.Go() = x.Helper()
    member this.CallOther(other: Calculator) = other.Helper()


  let createPoint: Point = { X = 1; Y = 2 }
  let convert (value: Point) : Result<Point, string> = Ok value

  let f (x: Foo) (xs: Foo list) y = y

  let local value = value + 1

  let callPoint point =
    local point
    System.Console.WriteLine(point.X)
    point.X

  let makeCalculator = Calculator(1)
  let literalString = "hello"
  let literalChar = 'x'
  let literalInt = 42
  let literalFloat = 3.14
  let literalDecimal = 1.5M
  let literalBool = true
  let literalUnit = ()

  let flow count =
    if count > 0 then
      match count with
      | 1 -> 1
      | n when n > 1 -> n
      | _ -> 0
    else
      try
        while count > 0 do
          ()
        0
      with
      | :? Exception -> -1
      | _ -> -2
