#[macro_use]
extern crate rocket;

use rocket::serde::json::Json;

#[get("/hello/<name>/<age>")]
fn hello(name: &str, age: u8) -> String {
    format!("{} {}", name, age)
}

#[post("/users", data = "<user>")]
fn create(user: Json<User>) -> Status {
    Status::Created
}

#[route(DELETE, uri = "/files/<path..>")]
fn remove(path: PathBuf) -> Status {
    Status::NoContent
}

#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount("/api", routes![hello, create])
        .mount("/admin", routes![remove])
}
