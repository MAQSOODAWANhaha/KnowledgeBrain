use api::{AppState, router_with};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use knowledge::Store;
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use uuid::Uuid;

fn redis_contract_required() -> bool {
    std::env::var("KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS").as_deref() == Ok("1")
}

async fn contract_pool() -> Option<PgPool> {
    let pool = match platform::connect().await {
        Ok(pool) => pool,
        Err(error) if std::env::var_os("DATABASE_URL").is_some() => {
            panic!("connect required Bid queue contract database: {error}")
        }
        Err(error) => {
            eprintln!("skip Bid queue contract: database unavailable: {error}");
            return None;
        }
    };
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure(
          'kb_bid_bind_matching_schedule_target(uuid,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("probe Bid queue contract schema");
    if !ready {
        if std::env::var_os("DATABASE_URL").is_some() {
            panic!("required Bid queue contract schema is unavailable");
        }
        eprintln!("skip Bid queue contract: final schema unavailable");
        return None;
    }
    sqlx::query(
        "UPDATE application_maintenance_gate
            SET mode='open',generation=generation+1,
                updated_by='system:first-launch',updated_at=clock_timestamp()
          WHERE singleton_key AND mode='maintenance'",
    )
    .execute(&pool)
    .await
    .expect("open Bid queue contract maintenance gate");
    Some(pool)
}

fn app() -> axum::Router {
    router_with(AppState {
        test_catalog: Some(Arc::new(Mutex::new(Store::default()))),
        jwt_secret: "bid-queue-contract-secret".into(),
        bootstrap_key: String::new(),
    })
}

fn schedule_request(token: &str, project_id: Uuid, key: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/v1/bids/{project_id}/matching/schedule"))
        .header("authorization", format!("Bearer {token}"))
        .header("idempotency-key", key)
        .body(Body::empty())
        .unwrap()
}

async fn call(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn insert_project(pool: &PgPool, project_id: Uuid, owner_id: Uuid) {
    sqlx::query(
        "INSERT INTO bid_projects
         (id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,
          matching_mutation_watermark,created_by)
         VALUES($1,'Bid queue contract',$2,clock_timestamp()+interval '30 days',
                repeat('0',64),repeat('1',64),1,$3)",
    )
    .bind(project_id)
    .bind(owner_id)
    .bind(format!("user:{owner_id}"))
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_schedule_intent(
    pool: &PgPool,
    project_id: Uuid,
    generation: i64,
    watermark: i64,
) -> Uuid {
    let intent_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO bid_matching_schedule_intents(
           id,project_id,generation,mutation_watermark,matching_config_snapshot_id,
           feature_snapshot_id,score_policy_snapshot_id,verifier_policy_snapshot_id)
         VALUES($1,$2,$3,$4,
           kb_bid_current_operation_snapshot('matching_config'),
           kb_bid_current_operation_snapshot('feature'),
           kb_bid_current_operation_snapshot('score_policy'),
           kb_bid_current_operation_snapshot('verifier_policy'))",
    )
    .bind(intent_id)
    .bind(project_id)
    .bind(generation)
    .bind(watermark)
    .execute(pool)
    .await
    .unwrap();
    intent_id
}

#[tokio::test]
async fn matching_schedule_503_retry_keeps_the_first_target_receipt() {
    let Some(pool) = contract_pool().await else {
        return;
    };
    let live_redis_url = match std::env::var("REDIS_URL") {
        Ok(value) => value,
        Err(error) if redis_contract_required() => {
            panic!("required Bid queue contract REDIS_URL is unavailable: {error}")
        }
        Err(error) => {
            eprintln!("skip Bid queue contract: REDIS_URL unavailable: {error}");
            return;
        }
    };
    let queue = platform::oxana_connect().expect("configure live Bid queue storage");
    let initial_queue_depth = queue
        .enqueued_count(platform::BidDeliveryV1Queue)
        .await
        .expect("connect live Bid queue");
    let owner_id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email) VALUES($1,$2)")
        .bind(owner_id)
        .bind(format!("bid-queue-{owner_id}@invalid.test"))
        .execute(&pool)
        .await
        .unwrap();
    let token = platform::issue_jwt(owner_id, "bid-queue-contract-secret").unwrap();
    let app = app();

    let project_id = Uuid::new_v4();
    insert_project(&pool, project_id, owner_id).await;
    let first_target = insert_schedule_intent(&pool, project_id, 1, 1).await;
    let key = format!("matching-delivery-{project_id}");
    unsafe {
        std::env::set_var("REDIS_URL", "redis://127.0.0.1:1/");
    }
    let unavailable = call(&app, schedule_request(&token, project_id, &key)).await;
    unsafe {
        std::env::set_var("REDIS_URL", &live_redis_url);
    }
    assert_eq!(unavailable.0, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(unavailable.1["error"]["code"], "QUEUE_UNAVAILABLE");
    assert_eq!(
        unavailable.1["error"]["target_id"],
        first_target.to_string()
    );
    assert_eq!(unavailable.1["error"]["target_revision"], 1);

    sqlx::query("UPDATE bid_projects SET matching_mutation_watermark=2 WHERE id=$1")
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
    let second_target = insert_schedule_intent(&pool, project_id, 2, 2).await;
    let replay = call(&app, schedule_request(&token, project_id, &key)).await;
    assert_eq!(replay.0, StatusCode::ACCEPTED, "{}", replay.1);
    assert_eq!(replay.1["job_id"], first_target.to_string());
    assert_eq!(replay.1["target_revision"], 1);
    assert_eq!(
        queue
            .enqueued_count(platform::BidDeliveryV1Queue)
            .await
            .unwrap(),
        initial_queue_depth + 1
    );

    let next_key = format!("matching-delivery-next-{project_id}");
    let next = call(&app, schedule_request(&token, project_id, &next_key)).await;
    assert_eq!(next.0, StatusCode::ACCEPTED, "{}", next.1);
    assert_eq!(next.1["job_id"], second_target.to_string());
    assert_eq!(next.1["target_revision"], 2);
    assert_eq!(
        queue
            .enqueued_count(platform::BidDeliveryV1Queue)
            .await
            .unwrap(),
        initial_queue_depth + 2
    );

    let empty_project_id = Uuid::new_v4();
    insert_project(&pool, empty_project_id, owner_id).await;
    let empty_key = format!("matching-delivery-empty-{empty_project_id}");
    let empty = call(&app, schedule_request(&token, empty_project_id, &empty_key)).await;
    assert_eq!(empty.0, StatusCode::OK, "{}", empty.1);
    assert!(empty.1["job_id"].is_null());
    let later_target = insert_schedule_intent(&pool, empty_project_id, 1, 1).await;
    let empty_replay = call(&app, schedule_request(&token, empty_project_id, &empty_key)).await;
    assert_eq!(empty_replay.0, StatusCode::OK, "{}", empty_replay.1);
    assert!(empty_replay.1["job_id"].is_null());
    assert_eq!(
        queue
            .enqueued_count(platform::BidDeliveryV1Queue)
            .await
            .unwrap(),
        initial_queue_depth + 2
    );
    let empty_next = call(
        &app,
        schedule_request(
            &token,
            empty_project_id,
            &format!("matching-delivery-empty-next-{empty_project_id}"),
        ),
    )
    .await;
    assert_eq!(empty_next.0, StatusCode::ACCEPTED, "{}", empty_next.1);
    assert_eq!(empty_next.1["job_id"], later_target.to_string());

    for (target_id, revision) in [(first_target, 1), (second_target, 2), (later_target, 1)] {
        queue
            .delete_unique_job(&platform::BidDeliveryV1Job::new(
                platform::BidDeliveryTargetKind::MatchingSchedule,
                target_id,
                revision,
            ))
            .await
            .expect("remove Bid queue contract job");
    }
    assert_eq!(
        queue
            .enqueued_count(platform::BidDeliveryV1Queue)
            .await
            .unwrap(),
        initial_queue_depth
    );
}
