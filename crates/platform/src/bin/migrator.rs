fn bootstrap_diagnostic(error: &platform::SchemaError) -> String {
    // Never log database messages, queries, details or connection strings: they may
    // contain caller data. Only SQLSTATE and numeric original-source position escape.
    match error {
        platform::SchemaError::Sql(sqlx::Error::Database(database)) => {
            let position = database
                .try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
                .and_then(|error| match error.position() {
                    Some(sqlx::postgres::PgErrorPosition::Original(position)) => Some(position),
                    _ => None,
                });
            format!(
                " SQLSTATE={} position={position:?}",
                database.code().unwrap_or_default()
            )
        }
        _ => String::new(),
    }
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    platform::init_tracing();
    let pool = platform::connect_unverified()
        .await
        .unwrap_or_else(|error| panic!("postgres bootstrap connection failed: {error}"));
    platform::apply_fresh_baseline(&pool)
        .await
        .unwrap_or_else(|error| {
            let diagnostic = bootstrap_diagnostic(&error);
            panic!("fresh schema bootstrap failed: {error}{diagnostic}")
        });
    tracing::info!("fresh schema bootstrap complete");
}

#[cfg(test)]
mod tests {
    #[test]
    fn bootstrap_diagnostic_does_not_expose_driver_message() {
        let error =
            platform::SchemaError::Sql(sqlx::Error::Protocol("private driver payload".into()));
        assert_eq!(super::bootstrap_diagnostic(&error), "");
        assert_eq!(error.to_string(), "postgres schema verification failed");
    }
}
