//! Encryption and authentication.
//!
//! This was moved in a separate crate to speed up incremental compilation.
use aes_gcm_siv::Aes256GcmSiv;
use aes_gcm_siv::Nonce;
use aes_gcm_siv::aead;
use aes_gcm_siv::aead::Aead;
use aes_gcm_siv::aead::Generate;
use aes_gcm_siv::aead::KeyInit;
use aes_gcm_siv::aead::array::Array;
use argon2::Argon2;
use argon2::password_hash::phc::SaltString;
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::Cookie;
use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use subtle::ConstantTimeEq;

fn constant_time_str_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Login {
    pub username: Option<String>,
    pub password: Option<String>,
}

fn verify_login(actual: &Login, received: &Login) -> bool {
    let username_eq = match (&actual.username, &received.username) {
        (Some(a), Some(e)) => constant_time_str_eq(a, e),
        _ => false,
    };
    let password_eq = match (&actual.password, &received.password) {
        (Some(a), Some(e)) => constant_time_str_eq(a, e),
        _ => false,
    };
    username_eq && password_eq
}

struct BlogliteKey {
    key: Aes256GcmSiv,
}

pub type Salt = [u8; 22];

impl BlogliteKey {
    fn new(salt: &Salt, password: &str) -> Self {
        // Salt can be public because it does not help the attacker.
        // It is only used to defend against rainbow tables.
        let argon2 = Argon2::default();
        let mut key = [0u8; 32];
        argon2
            .hash_password_into(password.as_bytes(), salt, &mut key)
            .unwrap();
        BlogliteKey {
            key: Aes256GcmSiv::new_from_slice(&key).unwrap(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Ciphertext {
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

fn encrypt_login(salt: &Salt, password: &str) -> Ciphertext {
    let plaintext = Utc::now().to_rfc3339();
    let key = BlogliteKey::new(salt, password);
    // Nonce should be unique per message.
    // let nonce = Aes256GcmSiv::generate_nonce().unwrap();
    let nonce = Nonce::generate();
    let ciphertext = key.key.encrypt(&nonce, plaintext.as_bytes()).unwrap();
    Ciphertext {
        nonce: nonce.into(),
        ciphertext,
    }
}

fn decrypt_login(salt: &Salt, password: &str, auth: &Ciphertext) -> Option<String> {
    let key = BlogliteKey::new(salt, password);
    let nonce: aead::Nonce<Aes256GcmSiv> = Array(auth.nonce);
    let ciphertext = auth.ciphertext.as_slice();
    let plaintext = match key.key.decrypt(&nonce, ciphertext) {
        Ok(plaintext) => plaintext,
        Err(e) => {
            // This can occur when the salt is incorrect. Should not happen in
            // production I think. Time will tell.
            tracing::warn!("failed to decrypt login: {}", e);
            return None;
        }
    };
    String::from_utf8(plaintext).ok()
}

#[test]
fn encryption_roundtrip() {
    let salt = b"nblVMlxYtvt0rxo3BML3zw";
    let password = "password";
    let before = Utc::now();
    let auth = encrypt_login(salt, password);
    let plaintext = decrypt_login(salt, password, &auth).unwrap();
    let issued = DateTime::parse_from_rfc3339(&plaintext).unwrap();
    assert!(before <= issued && issued <= Utc::now());
    assert!(session_is_valid(&plaintext, Utc::now()));
}

pub fn handle_logout(jar: CookieJar) -> CookieJar {
    jar.remove(Cookie::from("auth"))
}

// A 30-day lifetime balances convenience and security. Unlike standard
// implementations, a stolen token remains valid even after the user logs out,
// because logout only removes the browser's cookie. However, since the user is
// also the server admin, he or she can invalidate all existing tokens by
// changing the configured password and restarting the server.
const MAX_AGE_SEC: i64 = 30 * 24 * 60 * 60; // 30 days.

fn session_is_valid(plaintext: &str, now: DateTime<Utc>) -> bool {
    let Ok(issued) = DateTime::parse_from_rfc3339(plaintext) else {
        return false;
    };
    let age = now.signed_duration_since(issued);
    chrono::Duration::zero() <= age && age < chrono::Duration::seconds(MAX_AGE_SEC)
}

#[test]
fn session_expiry_boundaries() {
    let issued = DateTime::parse_from_rfc3339("2026-09-01T12:34:56Z")
        .unwrap()
        .with_timezone(&Utc);
    let plaintext = issued.to_rfc3339();
    let lifetime = chrono::Duration::seconds(MAX_AGE_SEC);
    assert!(session_is_valid(&plaintext, issued));
    assert!(session_is_valid(
        &plaintext,
        issued + lifetime - chrono::Duration::seconds(1)
    ));
    assert!(!session_is_valid(&plaintext, issued + lifetime));
    assert!(!session_is_valid(
        &plaintext,
        issued + lifetime + chrono::Duration::seconds(1)
    ));
    assert!(!session_is_valid(
        &plaintext,
        issued - chrono::Duration::seconds(1)
    ));
    assert!(!session_is_valid("invalid", issued));
}

#[test]
fn rejects_date_only_cookies() {
    // Old cookies stored only the login date. Reject that format so users must
    // log in again and receive a timestamped cookie with 30-day expiry.
    let now = DateTime::parse_from_rfc3339("2026-09-01T12:34:56Z")
        .unwrap()
        .with_timezone(&Utc);
    assert!(!session_is_valid("2026-09-01", now));
}

pub fn is_logged_in(salt: &Salt, login: &Login, jar: &CookieJar) -> bool {
    let cookie = jar.get("auth");
    match cookie {
        Some(cookie) => {
            let ciphertext = match serde_json::from_str(cookie.value()) {
                Ok(ciphertext) => ciphertext,
                Err(_) => {
                    return false;
                }
            };
            let key = match &login.password {
                Some(key) => key,
                None => {
                    tracing::warn!("admin password not set");
                    return false;
                }
            };
            let plaintext = match decrypt_login(salt, key, &ciphertext) {
                Some(plaintext) => plaintext,
                None => {
                    tracing::warn!(
                        "failed to decrypt login; probably a cookie that belongs to another salt"
                    );
                    return false;
                }
            };
            session_is_valid(&plaintext, Utc::now())
        }
        None => false,
    }
}

pub fn generate_salt() -> Salt {
    let salt = SaltString::generate();
    let salt: &str = salt.as_ref();
    salt.as_bytes().try_into().unwrap()
}

pub fn handle_login(
    salt: &Salt,
    actual: &Login,
    received: &Login,
    jar: CookieJar,
) -> Option<CookieJar> {
    if verify_login(actual, received) {
        let password = match &received.password {
            Some(password) => password,
            None => {
                tracing::warn!("admin password not set");
                return None;
            }
        };
        let ciphertext = encrypt_login(salt, password);
        let ciphertext = serde_json::to_string(&ciphertext).unwrap();
        // Secure ensures only HTTPS scheme (except on localhost).
        // Without secure, a man-in-the-middle could steal the cookie.
        // HttpOnly prevents the cookie from being accessed by JavaScript.
        // SameSite=Strict prevents the cookie from being sent in a cross-site request.
        let cookie = format!(
            "auth={ciphertext}; Max-Age={MAX_AGE_SEC}; \
            Secure; HttpOnly; SameSite=Strict;"
        );
        let cookie = Cookie::parse(cookie).unwrap();
        let updated_jar = jar.add(cookie);
        Some(updated_jar)
    } else {
        None
    }
}
