mod ap;
mod api;
pub mod data;
mod discovery;
mod files;
pub mod health;
mod highlight;
pub mod html;
mod math;
mod md;
mod pool;
mod search;
pub mod serve;
mod settings;
mod trigger;

use clap::Parser;

#[derive(Clone, Debug, Parser)]
pub struct ServeArgs {
    #[arg(long, env = "BLOGLITE_PRODUCTION")]
    pub production: bool,
    #[arg(long, env = "BLOGLITE_PORT", default_value = "3000")]
    pub port: u16,
    #[arg(
        long,
        env = "BLOGLITE_DATABASE_PATH",
        default_value = "/data/db.sqlite"
    )]
    pub database_path: String,
    /// The username for the admin interface as well as for WebFinger.
    #[arg(long, env = "BLOGLITE_USERNAME", default_value = "admin")]
    pub username: String,
    /// The password for the admin interface.
    #[arg(long, env = "BLOGLITE_PASSWORD")]
    pub password: Option<String>,
    /// Read the admin password from this file, overriding --password and BLOGLITE_PASSWORD.
    #[arg(long, env = "BLOGLITE_PASSWORD_FILE")]
    pub password_file: Option<std::path::PathBuf>,
    /// The domain name of the website.
    #[arg(long, env = "BLOGLITE_DOMAIN", default_value = "")]
    pub domain: String,
    /// The language of the website.
    #[arg(long, env = "BLOGLITE_HTML_LANG", default_value = "en")]
    pub html_lang: String,
    /// The logging level.
    #[arg(long, env = "BLOGLITE_LOG_LEVEL", default_value = "info")]
    pub log_level: String,

    /// The token for triggering GitHub Actions.
    #[arg(long, env = "BLOGLITE_TRIGGER_TOKEN")]
    pub trigger_token: Option<String>,
    /// The owner and repository for triggering GitHub Actions.
    #[arg(long, env = "BLOGLITE_TRIGGER_OWNER_REPO")]
    pub trigger_owner_repo: Option<String>,
    /// The host on which the workflow is triggered (defaults to GitHub).
    #[arg(
        long,
        env = "BLOGLITE_TRIGGER_HOST",
        default_value = "https://github.com"
    )]
    pub trigger_host: String,
    /// The branch in which the workflow is triggered (defaults to `main`).
    #[arg(long, env = "BLOGLITE_TRIGGER_BRANCH", default_value = "main")]
    pub trigger_branch: String,
    /// The workflow ID (filename) that is triggered (defaults to `backup.yml`).
    #[arg(
        long,
        env = "BLOGLITE_TRIGGER_WORKFLOW_ID",
        default_value = "backup.yml"
    )]
    pub trigger_workflow_id: String,
}

/// Read the admin password from file, env, or error when it fails.
pub fn resolve_password(args: &ServeArgs) -> std::io::Result<String> {
    let password = match &args.password_file {
        Some(path) => {
            if args.password.is_some() {
                eprintln!(
                    "Warning: --password or BLOGLITE_PASSWORD is set but will be ignored because a password file is configured"
                );
            }
            let mut password = std::fs::read_to_string(path).map_err(|err| {
                std::io::Error::new(
                    err.kind(),
                    format!("Could not read password file {}: {err}", path.display()),
                )
            })?;
            // Remove a single Unix or Windows line ending, preserving other whitespace.
            if password.ends_with('\n') {
                password.pop();
                if password.ends_with('\r') {
                    password.pop();
                }
            }
            password
        }
        None => match &args.password {
            Some(password) => password.clone(),
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Admin password not set",
                ));
            }
        },
    };
    if password.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Admin password must not be empty",
        ));
    }
    Ok(password)
}
