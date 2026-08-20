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

pub fn hash_password(password: &str) -> String {
    hex::encode(Sha256::digest(password.as_bytes()))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    hash_password(password) == hash
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
    domain::first_env(&["KNOWLEDGEBRAIN_LDAP_URL"])
}

/// No LDAP URL: local/test login. Do not check password; find-or-create the user.
pub fn local_open() -> bool {
    ldap_url().is_empty()
}

/// LDAP bind. `{user}` in bind DN is replaced by the login identifier (email local-part).
pub fn ldap_bind(user: &str, password: &str) -> Result<String, String> {
    let url = ldap_url();
    if url.is_empty() {
        return Err("ldap not configured".into());
    }
    let ident = user.split('@').next().unwrap_or(user);
    let dn_tpl = domain::first_env(&["KNOWLEDGEBRAIN_LDAP_BIND_DN"]);
    let dn = if dn_tpl.is_empty() {
        format!("uid={ident}")
    } else {
        dn_tpl.replace("{user}", ident).replace("{email}", user)
    };
    // Minimal LDAP simple-bind over TCP. Production can swap in a full client.
    let host = url
        .trim_start_matches("ldap://")
        .trim_start_matches("ldaps://")
        .split('/')
        .next()
        .unwrap_or("127.0.0.1:389");
    let addr = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:389")
    };
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
    if n > 8 && buf.windows(3).any(|w| w == [0x0a, 0x01, 0x00]) {
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
    fn jwt_roundtrip() {
        let id = Uuid::new_v4();
        let tok = issue_jwt(id, "secret").unwrap();
        assert_eq!(parse_jwt(&tok, "secret").unwrap(), id);
        assert!(parse_jwt(&tok, "other").is_err());
    }
}
