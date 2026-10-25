#[allow(dead_code)]
mod common;

use bloglite::ServeArgs;
use bloglite::resolve_password;
use common::TestDefault;
use std::path::PathBuf;
use std::process::Command;

struct SecretDirectory(PathBuf);

impl SecretDirectory {
    fn new() -> Self {
        static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "bloglite-password-test-{}-{unique}-{id}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for SecretDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn password_file_preserves_whitespace_and_overrides_password() {
    let dir = SecretDirectory::new();
    let path = dir.0.join("password");
    let args = ServeArgs::test_default();
    assert_eq!(resolve_password(&args).unwrap(), "test-password");
    let args = ServeArgs {
        password_file: Some(path.clone()),
        ..args
    };

    for (contents, expected) in [
        ("secret", "secret"),
        ("secret\n", "secret"),
        ("secret\r\n", "secret"),
        ("  secret \t\n", "  secret \t"),
        ("secret\n\n", "secret\n"),
        ("secret\r", "secret\r"),
    ] {
        std::fs::write(&path, contents).unwrap();
        assert_eq!(resolve_password(&args).unwrap(), expected);
        assert_eq!(args.password.as_deref(), Some("test-password"));
    }

    std::fs::write(&path, [0xff]).unwrap();
    assert_eq!(
        resolve_password(&args).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn unreadable_password_file_fails_before_database_creation() {
    let dir = SecretDirectory::new();
    let missing = dir.0.join("missing-secret");
    let database = dir.0.join("db.sqlite");
    for (use_env, source) in [
        (false, "cli"),
        (true, "env"),
        (true, "both"),
        (true, "none"),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bloglite"));
        command.env_clear().args(["serve", "--database-path"]);
        command.arg(&database);
        // Neither a CLI password nor an environment password should mask a file error.
        if source == "env" || source == "both" {
            command.env("BLOGLITE_PASSWORD", "env-secret");
        }
        if source == "cli" || source == "both" {
            command.args(["--password", "cli-secret"]);
        }
        if use_env {
            command.env("BLOGLITE_PASSWORD_FILE", &missing);
        } else {
            command.arg("--password-file").arg(&missing);
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("Could not read password file"));
        assert_eq!(
            stderr.contains("Warning: --password or BLOGLITE_PASSWORD is set but will be ignored"),
            source != "none"
        );
        assert!(!stderr.contains("cli-secret"));
        assert!(!stderr.contains("env-secret"));
        assert!(!database.exists());
    }
}

#[test]
fn missing_or_empty_password_fails_before_database_creation() {
    let dir = SecretDirectory::new();
    let path = dir.0.join("password");
    let database = dir.0.join("db.sqlite");
    for source in ["missing", "cli", "env", "file", "lf", "crlf"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bloglite"));
        command.env_clear().args(["serve", "--database-path"]);
        command.arg(&database);
        match source {
            "missing" => {}
            "cli" => {
                command.args(["--password", ""]);
            }
            "env" => {
                command.env("BLOGLITE_PASSWORD", "");
            }
            _ => {
                let contents = match source {
                    "lf" => "\n",
                    "crlf" => "\r\n",
                    _ => "",
                };
                std::fs::write(&path, contents).unwrap();
                command.env("BLOGLITE_PASSWORD_FILE", &path);
                // An empty file must not fall back to a direct password.
                command.env("BLOGLITE_PASSWORD", "fallback-secret");
            }
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1), "source: {source}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        let expected = if source == "missing" {
            "Admin password not set"
        } else {
            "Admin password must not be empty"
        };
        assert!(
            stderr.contains(expected),
            "source: {source}, stderr: {stderr}"
        );
        assert!(!stderr.contains("fallback-secret"));
        assert!(!database.exists());
    }
}
