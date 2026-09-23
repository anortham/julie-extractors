namespace Shop.Web

open System
open System.Net.Http
open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Mvc
open Giraffe

[<ApiController>]
[<Route("api/[controller]")>]
type UsersController(svc: IUserService) =
    inherit ControllerBase()

    [<HttpGet("{id}")>]
    member this.Get(id: int) = this.Ok(svc.Find id)

    [<HttpPost>]
    member this.Create() = this.Ok()

module App =
    let webApp =
        choose [
            GET >=> choose [
                route "/ping" >=> text "pong"
                routef "/orders/%i" getOrderHandler
                subRoute "/api" (choose [ route "/health" >=> text "ok" ]) ]
            POST >=> route "/orders" >=> createOrderHandler ]

    let configure (app: WebApplication) =
        app.MapGet("/hello", Func<string>(fun () -> "Hello")) |> ignore

    let fetch (client: HttpClient) = client.GetAsync("https://api.example.com/users")
