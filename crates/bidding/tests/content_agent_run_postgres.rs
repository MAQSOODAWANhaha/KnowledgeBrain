#[allow(dead_code)]
mod support;

use sqlx::{Executor, PgPool};

async fn owner(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut connection = pool.acquire().await.unwrap();
    connection.execute("SET ROLE kb_app_owner").await.unwrap();
    connection
}

#[tokio::test]
async fn outline_response_docx_and_export_acceptance_passes() {
    let Some(pool) = support::connect_postgres_contract("outline response acceptance").await else {
        return;
    };
    let mut connection = owner(&pool).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(include_str!(
        "sql/outline_response_acceptance.sql"
    )))
    .execute(&mut *connection)
    .await
    .expect("outline, response, docx, and export acceptance");
}
