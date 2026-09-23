namespace rec Shop.Core

/// An order could not be placed.
exception InvalidOrder of string

[<Measure>] type kg

type IShape =
    abstract Area: unit -> float

[<Interface>]
type IMarker =
    abstract Tag: string

[<Struct>]
type Point =
    val X: float
    new(x) = { X = x }

type Color =
    | Red = 0
    | Green = 1

type Repo(conn: string, timeout: int) =
    new() = Repo("local", 30)
    /// The repository name.
    member val Name = "repo" with get, set
    static member val Instances = 0 with get, set
    abstract member Label: string with get, set
    default val Label = "" with get, set
    member _.Connection = conn
    member this.Find(id: int) : string option = None
    member _.Join(left, right) : string = left + right
    static member (+) (a: Repo, b: Repo) = a

type Vec =
    { VX: float; VY: float }
    member this.Length = sqrt (this.VX * this.VX)

type Vec with
    member this.IsZero = this.VX = 0.0

type System.String with
    member this.Shout() = this.ToUpper()

module Patterns =
    let (|Even|Odd|) n = if n % 2 = 0 then Even else Odd
    let (|Positive|_|) n = if n > 0 then Some n else None
    let inline (+.) x y = x + y
    let [<Literal>] MaxSize = 100
    let classify = function
        | 0 -> "zero"
        | _ -> "other"
    let (num, label) = (1, "a")
    let describe n =
        match n with
        | Even -> "even"
        | Odd -> "odd"

module Work =
    let compute () =
        let a = 1
        let b = a + 1
        use stream = new System.IO.MemoryStream()
        a + b + int stream.Length
    let apply xs = List.map (fun item -> item + 1) xs
    let chained (s: string) = s.Trim().ToLower().Split(',')
    let first (xs: int[]) = xs[0]
    let tail (xs: int list) = xs[1..]
    let big = 10L
    let small = 3u
    let lookup (m: Map<string, int list>) = m
    let load () = task { return 1 }
    let quoted = <@ 1 + 1 @>
