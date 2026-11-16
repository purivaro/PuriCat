use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Mysql,
    Postgres,
    /// Declared so saved profiles stay forward compatible; no adapter yet.
    Sqlserver,
}

impl Engine {
    pub fn default_port(self) -> u16 {
        match self {
            Engine::Mysql => 3306,
            Engine::Postgres => 5432,
            Engine::Sqlserver => 1433,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    #[default]
    Development,
    Staging,
    Production,
}

/// TLS policy, named after the PostgreSQL `sslmode` values and mapped per engine.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TlsMode {
    Disable,
    /// Use TLS when the server offers it. Not protected against downgrade.
    #[default]
    Prefer,
    /// Encrypt, but do not verify the server certificate.
    Require,
    /// Encrypt and verify the certificate chain.
    VerifyCa,
    /// Encrypt, verify the chain and the host name.
    VerifyFull,
}

/// A saved connection. Secrets are *not* part of the profile; they live in the OS
/// credential store keyed by the profile id (see `secrets`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionProfile {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub engine: Engine,
    #[serde(default)]
    pub environment: Environment,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Default database. Optional for MySQL; PostgreSQL falls back to `postgres`.
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub tls: TlsMode,
    /// Extra CA bundle (PEM) used with `verify-ca` / `verify-full`.
    #[serde(default)]
    pub tls_ca_path: Option<String>,
    /// Ask the server to reject writes for every session PuriCat opens.
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_timeout")]
    pub connect_timeout_secs: u32,
    /// Saves touching more rows than this always go through the review step.
    #[serde(default = "default_review_rows")]
    pub bulk_review_rows: u32,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub last_used_at: Option<i64>,
}

fn default_timeout() -> u32 {
    10
}

fn default_review_rows() -> u32 {
    10
}

impl ConnectionProfile {
    /// Trim user input and reject profiles that cannot possibly connect.
    pub fn normalized(mut self) -> Result<Self> {
        self.name = self.name.trim().to_string();
        self.host = self.host.trim().to_string();
        self.username = self.username.trim().to_string();
        self.database = clean_opt(self.database);
        self.tls_ca_path = clean_opt(self.tls_ca_path);
        self.tags = self
            .tags
            .iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        self.tags.dedup();

        if self.name.is_empty() {
            return Err(Error::Invalid("Connection name is required".into()));
        }
        if self.host.is_empty() {
            return Err(Error::Invalid("Host is required".into()));
        }
        if self.port == 0 {
            return Err(Error::Invalid("Port must be between 1 and 65535".into()));
        }
        if self.username.is_empty() {
            return Err(Error::Invalid("Username is required".into()));
        }
        self.connect_timeout_secs = self.connect_timeout_secs.clamp(1, 120);
        Ok(self)
    }
}

fn clean_opt(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ConnectionProfile {
        serde_json::from_value(serde_json::json!({
            "name": "  Local PG ", "engine": "postgres", "host": " localhost ",
            "port": 5432, "username": "app", "database": "  ",
            "tags": ["a", " ", " b "]
        }))
        .unwrap()
    }

    #[test]
    fn defaults_are_safe_and_input_is_trimmed() {
        let p = sample().normalized().unwrap();
        assert_eq!(p.name, "Local PG");
        assert_eq!(p.host, "localhost");
        assert_eq!(p.database, None);
        assert_eq!(p.tags, vec!["a", "b"]);
        assert_eq!(p.environment, Environment::Development);
        assert_eq!(p.tls, TlsMode::Prefer);
        assert_eq!(p.connect_timeout_secs, 10);
        assert_eq!(p.bulk_review_rows, 10);
    }

    #[test]
    fn rejects_incomplete_profiles() {
        let mut p = sample();
        p.host = " ".into();
        assert!(matches!(p.normalized(), Err(Error::Invalid(_))));
        let mut p = sample();
        p.port = 0;
        assert!(p.normalized().is_err());
    }

    #[test]
    fn wire_format_is_stable() {
        let json = serde_json::to_value(sample().normalized().unwrap()).unwrap();
        assert_eq!(json["engine"], "postgres");
        assert_eq!(json["tls"], "prefer");
        assert_eq!(json["readOnly"], false);
        let tls: TlsMode = serde_json::from_str("\"verify-full\"").unwrap();
        assert_eq!(tls, TlsMode::VerifyFull);
    }
}
