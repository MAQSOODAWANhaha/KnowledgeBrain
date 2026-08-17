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
