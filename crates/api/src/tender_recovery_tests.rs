//! Recovery routes authenticate before accessing the bridge or original task.
use super::*;
use axum::body::Body;
use tower::ServiceExt;

#[tokio::test]
async fn analysis_recovery_routes_require_project_authentication() {
    let state = AppState {
        jwt_secret: "synthetic-recovery-secret".into(),
        bootstrap_key: String::new(),
    };
    let project = Uuid::new_v4();
    for (method, suffix) in [("GET", "recovery"), ("POST", "retry")] {
        let response = router()
            .with_state(state.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri(format!(
                        "/api/v3/tender-projects/{project}/analysis/{suffix}"
                    ))
                    .header("content-type", "application/json")
                    .header("idempotency-key", "same-recovery")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
