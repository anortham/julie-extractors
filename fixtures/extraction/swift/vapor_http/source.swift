import Vapor
import Alamofire

func routes(_ app: Application) throws {
    app.get("hello") { req async -> String in "Hello" }
    let api = app.grouped("api", "v1")
    api.post("todos", use: createTodo)
    app.group("admin") { admin in
        admin.delete("users", ":id", use: removeUser)
    }
    app.on(.PATCH, "settings", use: updateSettings)
}

struct TodoController: RouteCollection {
    func boot(routes: RoutesBuilder) throws {
        let todos = routes.grouped("todos")
        todos.get(use: index)
        todos.delete(":todoID", use: delete)
    }
}

func fetchRemote() async throws {
    AF.request("https://api.example.com/todos", method: .post).responseJSON { _ in }
    let task = URLSession.shared.dataTask(with: URL(string: "https://api.example.com/users")!) { _, _, _ in }
    task.resume()
    let (data, _) = try await URLSession.shared.data(for: URLRequest(url: URL(string: "https://api.example.com/orders")!))
    _ = data
}
