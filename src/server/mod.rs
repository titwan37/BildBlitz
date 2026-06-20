// src/server/mod.rs

use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use crate::library::db::DatabaseManager;
use serde::Serialize;

#[derive(Serialize)]
struct Stats {
    total_images: i64,
    total_collections: i64,
}

#[get("/stats")]
async fn get_stats(db: web::Data<DatabaseManager>) -> impl Responder {
    let pool = db.get_pool();
    
    let image_count: i64 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM images")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        
    let collection_count: i64 = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM virtual_collections")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    HttpResponse::Ok().json(Stats {
        total_images: image_count,
        total_collections: collection_count,
    })
}

#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().body("BildBlitz API is running. Try /stats")
}

pub fn start_server(db: DatabaseManager) -> std::io::Result<actix_web::dev::Server> {
    let db_data = web::Data::new(db);
    
    info!("Starting web server on 127.0.0.1:8080");
    
    let server = HttpServer::new(move || {
        App::new()
            .app_data(db_data.clone())
            .service(index)
            .service(get_stats)
    })
    .bind(("127.0.0.1", 8080))?
    .run();
    
    Ok(server)
}

use tracing::info;
