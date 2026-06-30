using Microsoft.AspNetCore.Builder;

var builder = WebApplication.CreateBuilder(args);
var app = builder.Build();

app.MapGet("/todos", () => "ok");
app.MapPost("/todos", CreateTodo);
app.MapDelete("/todos/{id}", DeleteTodo);

var admin = app.MapGroup("/admin/connectors");
admin.MapPost("/save", CreateTodo);

var dynamicRoute = "/dynamic";
app.MapGet(dynamicRoute, () => "skip");
app.MapGet($"/computed/{id}", () => "skip");

static IResult CreateTodo() => Results.Ok();
static IResult DeleteTodo(int id) => Results.Ok();
