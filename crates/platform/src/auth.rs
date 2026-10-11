use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use chrono::{Duration, Utc};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
}

/// Stable digest for high-entropy API keys, not human passwords.
pub fn hash_password(password: &str) -> String {
    hex::encode(Sha256::digest(password.as_bytes()))
}

pub fn hash_user_password(password: &str) -> Result<String, String> {
    // UUID v4 uses operating-system randomness. Each account receives a fresh salt.
    let salt =
        SaltString::encode_b64(Uuid::new_v4().as_bytes()).map_err(|_| "password salt failed")?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "password hashing failed".into())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(hash) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .is_ok()
}

/// Secrets retain their exact bytes; blank values are never valid defaults.
pub fn required_secret(name: &str) -> Result<String, String> {
    require_secret_value(name, std::env::var(name).ok())
}

fn require_secret_value(name: &str, value: Option<String>) -> Result<String, String> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

pub struct LocalAdminConfig {
    pub username: String,
    password: String,
}

impl LocalAdminConfig {
    pub fn from_env() -> Result<Option<Self>, String> {
        let url = ldap_url();
        if !url.is_empty() {
            ldap_address(&url)?;
            return Ok(None);
        }
        Self::from_values(
            std::env::var("KB_ADMIN_USERNAME").ok(),
            std::env::var("KB_ADMIN_PASSWORD").ok(),
        )
        .map(Some)
    }

    fn from_values(username: Option<String>, password: Option<String>) -> Result<Self, String> {
        let username = require_secret_value("KB_ADMIN_USERNAME", username)?
            .trim()
            .to_owned();
        let password = require_secret_value("KB_ADMIN_PASSWORD", password)?;
        Ok(Self { username, password })
    }
}

/// Called only after schema/namespace verification. Never resets an existing account.
/// A nonempty database with a different identity is not a fresh local deployment.
pub async fn initialize_local_admin(
    pool: &sqlx::PgPool,
    config: &LocalAdminConfig,
) -> Result<(), String> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| "local account initialization failed")?;
    initialize_local_admin_in_transaction(&mut tx, config).await?;
    tx.commit()
        .await
        .map_err(|_| "local account initialization failed".into())
}

async fn initialize_local_admin_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    config: &LocalAdminConfig,
) -> Result<(), String> {
    sqlx::query("SELECT pg_advisory_xact_lock(710563092214)")
        .execute(&mut **tx)
        .await
        .map_err(|_| "local account initialization lock failed")?;
    let existing: Option<Option<String>> =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE email=$1")
            .bind(&config.username)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| "local account lookup failed")?;
    if let Some(hash) = existing {
        if !hash
            .as_deref()
            .is_some_and(|hash| hash.starts_with("$argon2id$") && PasswordHash::new(hash).is_ok())
        {
            return Err(
                "existing account is not a supported local administrator; use a fresh database"
                    .into(),
            );
        }
    } else {
        let occupied: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users)")
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| "local account lookup failed")?;
        if occupied {
            return Err(
                "KB_ADMIN_USERNAME differs from the initialized account; use the original identity"
                    .into(),
            );
        }
        let password = config.password.clone();
        let hash = tokio::task::spawn_blocking(move || hash_user_password(&password))
            .await
            .map_err(|_| "password hashing failed")??;
        sqlx::query("INSERT INTO users (id,email,password_hash) VALUES ($1,$2,$3)")
            .bind(Uuid::new_v4())
            .bind(&config.username)
            .bind(hash)
            .execute(&mut **tx)
            .await
            .map_err(|_| "local account initialization failed")?;
    }
    Ok(())
}

pub fn issue_jwt(user_id: Uuid, secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let exp = (Utc::now() + Duration::hours(24)).timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        exp,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn ldap_url() -> String {
    crate::first_env(&["KNOWLEDGEBRAIN_LDAP_URL"])
}

fn ldap_address(url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "invalid KNOWLEDGEBRAIN_LDAP_URL")?;
    if parsed.scheme() == "ldaps" {
        return Err("LDAPS is not supported by this build; TLS will not be downgraded".into());
    }
    if parsed.scheme() != "ldap" || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(
            "KNOWLEDGEBRAIN_LDAP_URL must be an explicit ldap:// endpoint without credentials"
                .into(),
        );
    }
    let host = parsed
        .host_str()
        .ok_or("KNOWLEDGEBRAIN_LDAP_URL requires a host")?;
    Ok(format!("{host}:{}", parsed.port().unwrap_or(389)))
}

fn escape_ldap_dn_value(value: &str) -> String {
    let last = value.chars().count().saturating_sub(1);
    let mut escaped = String::new();
    for (index, character) in value.chars().enumerate() {
        if character == '\0' {
            escaped.push_str("\\00");
        } else {
            if matches!(character, ',' | '+' | '"' | '\\' | '<' | '>' | ';')
                || (index == 0 && character == '#')
                || (character == ' ' && (index == 0 || index == last))
            {
                escaped.push('\\');
            }
            escaped.push(character);
        }
    }
    escaped
}

/// LDAP bind. `{user}` in bind DN is replaced by the login identifier (email local-part).
pub fn ldap_bind(user: &str, password: &str) -> Result<String, String> {
    let url = ldap_url();
    if url.is_empty() {
        return Err("ldap not configured".into());
    }
    // LDAP simple bind with an empty password may authenticate anonymously.
    if user.trim().is_empty() || password.is_empty() {
        return Err("ldap credentials are required".into());
    }
    let addr = ldap_address(&url)?;
    let ident = escape_ldap_dn_value(user.split('@').next().unwrap_or(user));
    let dn_tpl = crate::first_env(&["KNOWLEDGEBRAIN_LDAP_BIND_DN"]);
    let dn = if dn_tpl.is_empty() {
        format!("uid={ident}")
    } else {
        dn_tpl
            .replace("{user}", &ident)
            .replace("{email}", &escape_ldap_dn_value(user))
    };
    // The legacy minimal client only supports short-form BER lengths.
    // Reject oversized inputs rather than truncating their encoded lengths.
    if dn.len() + password.len() > 110 {
        return Err("LDAP bind credentials exceed this client's supported length".into());
    }
    let mut stream = std::net::TcpStream::connect(&addr).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(8)))
        .ok();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(8)))
        .ok();
    let bind = ldap_simple_bind_packet(&dn, password);
    use std::io::{Read, Write};
    stream.write_all(&bind).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 256];
    let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
    if n >= 12
        && buf[0] == 0x30
        && buf[2..6] == [0x02, 0x01, 0x01, 0x61]
        && buf[7..10] == [0x0a, 0x01, 0x00]
    {
        Ok(dn)
    } else {
        Err("ldap bind failed".into())
    }
}

fn ldap_simple_bind_packet(dn: &str, password: &str) -> Vec<u8> {
    let mut inner = vec![0x02, 0x01, 0x03, 0x04, dn.len() as u8];
    inner.extend(dn.as_bytes());
    inner.push(0x80);
    inner.push(password.len() as u8);
    inner.extend(password.as_bytes());
    let mut bind = vec![0x02, 0x01, 0x01, 0x60, inner.len() as u8];
    bind.extend(inner);
    let mut out = vec![0x30, bind.len() as u8];
    out.extend(bind);
    out
}

pub fn parse_jwt(token: &str, secret: &str) -> Result<Uuid, String> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| e.to_string())?;
    Uuid::parse_str(&data.claims.sub).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_passwords_use_salted_argon2_and_reject_legacy_hashes() {
        let password = "synthetic password fixture";
        let first = hash_user_password(password).unwrap();
        let second = hash_user_password(password).unwrap();
        assert!(first.starts_with("$argon2id$"));
        assert_ne!(first, second);
        assert!(verify_password(password, &first));
        assert!(!verify_password("wrong fixture", &first));
        assert!(!verify_password(password, &hash_password(password)));
        assert!(!verify_password(password, ""));
    }

    #[test]
    fn required_credentials_reject_blank_and_preserve_secret_bytes() {
        for value in [None, Some(String::new()), Some("  ".into())] {
            assert!(require_secret_value("JWT_SECRET", value).is_err());
        }
        assert_eq!(
            require_secret_value("JWT_SECRET", Some(" exact fixture ".into())).unwrap(),
            " exact fixture "
        );
        assert!(LocalAdminConfig::from_values(None, Some("fixture".into())).is_err());
        assert!(LocalAdminConfig::from_values(Some("admin".into()), None).is_err());
        let config =
            LocalAdminConfig::from_values(Some(" admin ".into()), Some(" exact fixture ".into()))
                .unwrap();
        assert_eq!(config.username, "admin");
        assert_eq!(config.password, " exact fixture ");
    }

    #[test]
    fn ldap_identifiers_cannot_inject_distinguished_name_components() {
        assert_eq!(escape_ldap_dn_value("admin,ou=staff"), r"admin\,ou=staff");
        assert_eq!(escape_ldap_dn_value(" #admin "), r"\ #admin\ ");
        assert_eq!(escape_ldap_dn_value("a\0b"), r"a\00b");
    }

    #[test]
    fn ldap_never_downgrades_tls_or_accepts_another_scheme() {
        assert!(
            ldap_address("ldaps://localhost:636")
                .unwrap_err()
                .contains("will not be downgraded")
        );
        assert!(ldap_address("https://localhost").is_err());
        assert!(ldap_address("ldap://user:fixture@localhost").is_err());
        assert_eq!(ldap_address("ldap://localhost").unwrap(), "localhost:389");
    }

    #[tokio::test]
    #[ignore = "requires explicitly supplied disposable AUTH_TEST_DATABASE_URL"]
    async fn local_admin_initialization_is_idempotent_and_never_resets_passwords() {
        let url = std::env::var("AUTH_TEST_DATABASE_URL")
            .expect("disposable AUTH_TEST_DATABASE_URL required");
        let options = url.parse::<sqlx::postgres::PgConnectOptions>().unwrap();
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_port(), 25433);
        assert!(
            options
                .get_database()
                .is_some_and(|name| name.starts_with("knowledgebrain_test_"))
        );
        // Explicit disposable baseline and an outer rollback; no TEMP privilege needed.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .max_lifetime(None)
            .idle_timeout(None)
            .connect(&url)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(before, 0, "auth test needs a fresh empty users table");
        let config = LocalAdminConfig::from_values(
            Some("fixture-admin".into()),
            Some("first fixture password".into()),
        )
        .unwrap();
        initialize_local_admin_in_transaction(&mut tx, &config)
            .await
            .unwrap();
        let first: String = sqlx::query_scalar("SELECT password_hash FROM users")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(verify_password("first fixture password", &first));
        let changed = LocalAdminConfig::from_values(
            Some("fixture-admin".into()),
            Some("replacement fixture password".into()),
        )
        .unwrap();
        initialize_local_admin_in_transaction(&mut tx, &changed)
            .await
            .unwrap();
        let after: String = sqlx::query_scalar("SELECT password_hash FROM users")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(first, after);
        assert!(!verify_password("replacement fixture password", &after));
        let another =
            LocalAdminConfig::from_values(Some("another-admin".into()), Some("fixture".into()))
                .unwrap();
        assert!(
            initialize_local_admin_in_transaction(&mut tx, &another)
                .await
                .is_err()
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(count, 1);
        tx.rollback().await.unwrap();
        pool.close().await;
    }

    #[test]
    fn jwt_roundtrip() {
        let id = Uuid::new_v4();
        let tok = issue_jwt(id, "secret").unwrap();
        assert_eq!(parse_jwt(&tok, "secret").unwrap(), id);
        assert!(parse_jwt(&tok, "other").is_err());
    }
}
