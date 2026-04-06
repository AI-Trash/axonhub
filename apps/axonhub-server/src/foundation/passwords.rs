use bcrypt::{hash, verify, DEFAULT_COST};
use getrandom::getrandom;
use hex::encode as hex_encode;

pub(crate) fn hash_password(password: &str) -> Result<String, String> {
    hash(password, DEFAULT_COST)
        .map(|hashed| hex_encode(hashed.as_bytes()))
        .map_err(|error| error.to_string())
}

pub(crate) fn verify_password(stored_hex: &str, password: &str) -> bool {
    hex::decode(stored_hex)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|hash| verify(password, &hash).ok())
        .unwrap_or(false)
}

pub(crate) fn generate_secret_key() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom(&mut bytes).map_err(|error| error.to_string())?;
    Ok(hex_encode(bytes))
}
