//! HTTP: auth, catalog, ingest, retrieve. No parse / split / vector work here.

pub mod bid_v2_routes;
mod err;
mod routes;

use axum::Router;

#[derive(Clone)]
pub struct AppState {
    pub jwt_secret: String,
    pub bootstrap_key: String,
}

pub fn router() -> Router {
    router_with(AppState {
        jwt_secret: std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".into()),
        bootstrap_key: std::env::var("KNOWLEDGEBRAIN_BOOTSTRAP_KEY").unwrap_or_default(),
    })
}

pub fn router_with(state: AppState) -> Router {
    routes::build(state)
}

pub fn bind_addr() -> String {
    let port = std::env::var("API_PORT").unwrap_or_else(|_| "8080".into());
    format!("0.0.0.0:{port}")
}

#[derive(serde::Serialize)]
pub struct HealthBody {
    pub status: &'static str,
    pub service: &'static str,
}
