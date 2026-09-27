use actix_web::{App, web};

fn configure(prefix: &str, branch: bool) {
    let scope = web::scope("/api");
    scope.route("/users", web::get().to(list_users));
    {
        let scope = web::scope("/admin");
        scope.route("/health", web::get().to(admin_health));
    }
    let mut changing = web::scope("/stable");
    if branch {
        changing = web::scope(prefix);
    }
    changing.route("/items", web::get().to(list_items));
}

fn routes(config: &mut web::ServiceConfig) {
    config.route("/status", web::get().to(status));
}

fn app() {
    App::new()
        .service(web::scope("/api").configure(routes))
        .service(web::scope("/v2").configure(routes));
}
