namespace Shop

exception NotFound of string
exception Conflict of id: int * reason: string

type Payment =
    | Cash of decimal
    | Transfer of Account
and Account = { Iban: string }
and internal Bank() =
    let mutable visits = 0
    member _.Name = "b"

type private Hidden = { Secret: int }

type Line =
    { Qty: int; Price: decimal }
    member this.Total = decimal this.Qty * this.Price

module Pricing =
    open System

    let private fee = 2m
    let internal round2 (value: decimal) = Math.Round(value, 2)
    let greeting = String.Join(", ", [ "a"; "b" ])

    let parse (text: string) = int text
    let validate value = value > 0
    let check text = text |> parse |> validate
    let checkBack text = validate <| parse text
    let addFee (payment: Payment) = 1m + round2 fee

    let total (payment: Payment) =
        match payment with
        | Cash amount -> amount + fee
        | Transfer _ -> fee

    let rec countNode (depth: int) = 1 + countForest (depth - 1)
    and countForest depth = if depth <= 0 then 0 else countNode depth

    let find key = raise (NotFound key)

    let services (collection: IServiceCollection) =
        collection.AddSingleton<IClock, SystemClock>() |> ignore
