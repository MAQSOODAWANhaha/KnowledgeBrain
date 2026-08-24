use sqlx::PgPool;
use uuid::Uuid;

pub fn postgres_contract_tests_required() -> bool {
    std::env::var("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").is_ok_and(|value| value == "1")
        || std::env::var_os("DATABASE_URL").is_some()
}

pub async fn connect_postgres_contract(label: &str) -> Option<PgPool> {
    let database_url = match storage::database_url() {
        Ok(database_url) => database_url,
        Err(error) if postgres_contract_tests_required() => {
            panic!("read required {label} contract database URL: {error}")
        }
        Err(error) => {
            eprintln!("skipped {label} contract: database URL unavailable: {error}");
            return None;
        }
    };
    match PgPool::connect(&database_url).await {
        Ok(pool) => {
            open_test_maintenance_gate(&pool, label).await;
            Some(pool)
        }
        Err(error) if postgres_contract_tests_required() => {
            panic!("connect required {label} contract database: {error}")
        }
        Err(error) => {
            eprintln!("skipped {label} contract: database unavailable: {error}");
            None
        }
    }
}

async fn open_test_maintenance_gate(pool: &PgPool, label: &str) {
    let generation: Option<i64> = sqlx::query_scalar(
        "WITH changed AS (
           UPDATE application_maintenance_gate
              SET mode='open',generation=generation+1,
                  updated_by='system:first-launch',updated_at=clock_timestamp()
            WHERE singleton_key AND mode='maintenance'
            RETURNING generation
         ), audited AS (
           INSERT INTO maintenance_gate_audit(
             id,from_mode,to_mode,generation,actor_identity,reason)
           SELECT $1,'maintenance','open',generation,'system:first-launch',$2 FROM changed
         )
         SELECT generation FROM changed",
    )
    .bind(Uuid::new_v4())
    .bind(format!("open isolated {label} contract fixture"))
    .fetch_optional(pool)
    .await
    .unwrap_or_else(|error| panic!("open {label} contract maintenance gate: {error}"));
    if generation.is_none() {
        let mode: String =
            sqlx::query_scalar("SELECT mode FROM application_maintenance_gate WHERE singleton_key")
                .fetch_one(pool)
                .await
                .unwrap_or_else(|error| panic!("read {label} contract maintenance gate: {error}"));
        assert_eq!(mode, "open", "{label} contract requires an open test gate");
    }
}

pub fn require_final_schema(label: &str, ready: bool) -> bool {
    if ready {
        return true;
    }
    if postgres_contract_tests_required() {
        panic!("required final {label} schema unavailable");
    }
    eprintln!("skipped {label} contract: final schema unavailable");
    false
}
