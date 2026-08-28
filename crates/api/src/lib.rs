//! HTTP: auth, catalog, ingest, retrieve. No parse / split / vector work here.

mod bid_routes;
pub mod bid_v2_routes;
mod err;
mod routes;

use axum::Router;
use knowledge::Store;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    /// In-memory catalog for tests (`http_flow`). Production is `None`:
    /// each request hydrates a local Store from Postgres and discards it.
    pub test_catalog: Option<Arc<Mutex<Store>>>,
    pub jwt_secret: String,
    pub bootstrap_key: String,
}

pub fn router() -> Router {
    router_with(AppState {
        test_catalog: None,
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
