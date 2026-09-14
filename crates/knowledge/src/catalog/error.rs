//! Persistence error used by product delete/default library.
#[derive(Debug)]
pub enum PersistError {
    DefaultLibrary,
    NotFound,
    Sql(sqlx::Error),
}

impl From<sqlx::Error> for PersistError {
    fn from(e: sqlx::Error) -> Self {
        Self::Sql(e)
    }
}
