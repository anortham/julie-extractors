use actix_web::{App, guard, web};

fn configure(prefix: &str) {
    App::new()
        .service(
            web::scope("/api").service(
                web::scope("/v2").service(
                    web::resource("/users")
                        .guard(guard::Get())
                        .route(web::route().to(list_users)),
                ),
            ),
        )
        .service(
            web::scope(prefix)
                .service(web::resource("/dynamic").route(web::get().to(dynamic_route))),
        );
}
