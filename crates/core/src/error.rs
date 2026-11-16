use serde::ser::SerializeStruct;

pub type Result<T> = std::result::Result<T, Error>;

/// Errors surfaced to the UI. Messages must never contain secrets: adapters build
/// connections from typed options (never URLs), so driver errors carry no password.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The request itself is wrong (bad input, unknown column, ...).
    #[error("{0}")]
    Invalid(String),
    #[error("{0} not found")]
    NotFound(String),
    /// The target database server rejected or failed an operation.
    #[error("{0}")]
    Database(String),
    /// PuriCat's own settings database failed.
    #[error("local settings store: {0}")]
    Store(String),
    /// The OS credential store failed.
    #[error("credential store: {0}")]
    Secret(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Invalid(_) => "invalid",
            Error::NotFound(_) => "not_found",
            Error::Database(_) => "database",
            Error::Store(_) => "store",
            Error::Secret(_) => "secret",
            Error::Unsupported(_) => "unsupported",
            Error::Io(_) => "io",
        }
    }

    /// Map a driver error from a *remote* database.
    pub(crate) fn database(e: sqlx::Error) -> Self {
        Error::Database(e.to_string())
    }

    /// Map a driver error from the local SQLite store.
    pub(crate) fn store(e: sqlx::Error) -> Self {
        Error::Store(e.to_string())
    }
}

impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("Error", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}
