#r "nuget: FSharp.Data, 6.4.0"
#load "helpers.fsx"

open System

let scriptValue: int = 7

let scriptMain name =
  Console.WriteLine(name)
  scriptValue

// script comment
