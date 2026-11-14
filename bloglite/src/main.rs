use bloglite::ServeArgs;
use bloglite::health::HealthArgs;
use clap::Parser;
use tracing::Level;
use tracing::subscriber::SetGlobalDefaultError;

#[derive(Debug, clap::Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Task {
    /// Run a health check on the given port.
    CheckHealth(HealthArgs),
    /// Print the project's license.
    License,
    /// Preview a local Markdown draft and refresh when it changes.
    Preview {
        /// Path to the Markdown draft.
        draft: std::path::PathBuf,
        /// Local port (use 0 to pick a free port).
        #[arg(long, default_value_t = 3000)]
        port: u16,
        /// Live blog host for /files/ URLs (defaults to HTTPS).
        #[arg(long)]
        host: Option<String>,
    },
    /// Start the server.
    Serve(ServeArgs),
}

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    /// Whether to print verbose logs.
    #[arg(long)]
    verbose: bool,
    /// Whether to use ANSI colors.
    #[arg(long)]
    ansi: Option<bool>,
    #[command(subcommand)]
    task: Task,
}

/// Initialize logging with the given level.
pub fn init_subscriber(level: Level, ansi: bool) -> Result<(), SetGlobalDefaultError> {
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(level)
        .with_test_writer()
        .with_target(false)
        .with_ansi(ansi)
        // Write logs to stderr to allow writing sha output to stdout.
        .with_writer(std::io::stderr)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    match &args.task {
        Task::CheckHealth(args) => {
            bloglite::health::check_health(args).await;
        }
        Task::License => {
            let license_content = include_str!("../../LICENSE");
            println!("{}", license_content);
        }
        Task::Preview { draft, port, host } => {
            if let Err(err) = bloglite::preview::run(draft.clone(), *port, host.as_deref()).await {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        Task::Serve(serve_args) => {
            let password = match bloglite::resolve_password(serve_args) {
                Ok(password) => password,
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(1);
                }
            };
            let log_level = match serve_args.log_level.as_str() {
                "error" => Level::ERROR,
                "warn" => Level::WARN,
                "info" => Level::INFO,
                "debug" => Level::DEBUG,
                _ => Level::INFO,
            };
            init_subscriber(log_level, args.ansi.unwrap_or(true)).unwrap();
            bloglite::serve::run(serve_args, password).await;
        }
    }
}
