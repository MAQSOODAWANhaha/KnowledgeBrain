fn postgres_diagnostic(error: &sqlx::Error) -> String {
    // Never log database messages, queries, details or connection strings: they may
    // contain caller data. Only SQLSTATE and numeric original-source position escape.
    match error {
        sqlx::Error::Database(database) => {
            let position = database
                .try_downcast_ref::<sqlx::postgres::PgDatabaseError>()
                .and_then(|error| match error.position() {
                    Some(sqlx::postgres::PgErrorPosition::Original(position)) => Some(position),
                    _ => None,
                });
            format!(
                "postgres operation failed SQLSTATE={} position={position:?}",
                database.code().unwrap_or_default()
            )
        }
        _ => "postgres operation failed".into(),
    }
}

fn migration_diagnostic(error: &sqlx::migrate::MigrateError) -> String {
    use sqlx::migrate::MigrateError;
    match error {
        MigrateError::Execute(error) => postgres_diagnostic(error),
        MigrateError::ExecuteMigration(error, version) => {
            format!("version={version} {}", postgres_diagnostic(error))
        }
        MigrateError::VersionMissing(version) => {
            format!("applied version={version} is missing from migration files")
        }
        MigrateError::VersionMismatch(version) => {
            format!("applied version={version} checksum differs from migration file")
        }
        MigrateError::Dirty(version) => format!("version={version} is partially applied"),
        _ => "migration validation failed".into(),
    }
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    platform::init_tracing();
    let pool = platform::connect().await.unwrap_or_else(|error| {
        panic!(
            "migration connection failed: {}",
            postgres_diagnostic(&error)
        )
    });
    platform::migrate_database(&pool)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "database migration failed: {}",
                migration_diagnostic(&error)
            )
        });
    tracing::info!("database migrations complete");
}

#[cfg(test)]
mod tests {
    #[test]
    fn migration_diagnostic_does_not_expose_driver_message() {
        let error = sqlx::migrate::MigrateError::ExecuteMigration(
            sqlx::Error::Protocol("private driver payload".into()),
            20261028000001,
        );
        assert_eq!(
            super::migration_diagnostic(&error),
            "version=20261028000001 postgres operation failed"
        );
    }
}
