//! brains-server binary: parse CLI flags and run the library server.

use clap::Parser;

use brains_server::ServerConfig;

#[derive(Parser)]
#[command(name = "brains-server", version = brains_build_info::VERSION, about = "Brains local server")]
struct Args {
    /// Durable data directory (store, index db, auth registry, signing keys).
    #[arg(long, env = "BRAINS_DATA_DIR", default_value = "./brain-data")]
    data_dir: std::path::PathBuf,
    /// Vault name inside the store; one instance manages one vault.
    #[arg(long, env = "BRAINS_VAULT", default_value = "default")]
    vault: String,
    #[arg(long, env = "BRAINS_BIND", default_value = "127.0.0.1")]
    bind: String,
    #[arg(long, env = "BRAINS_PORT", default_value_t = 45800)]
    port: u16,
    /// Register (or replace) a whole-vault root token. Generated once if omitted.
    #[arg(
        long,
        env = "BRAINS_TOKEN",
        hide_env_values = true,
        conflicts_with = "token_file"
    )]
    token: Option<String>,
    /// Read the root token from a UTF-8 file (e.g. a Docker secret).
    #[arg(long, env = "BRAINS_TOKEN_FILE")]
    token_file: Option<std::path::PathBuf>,
    /// Start an empty vault, suitable for exact archive restoration.
    #[arg(long, env = "BRAINS_NO_SEED")]
    no_seed: bool,
    /// Serve a built web app (directory with index.html) from the same origin.
    #[arg(long, env = "BRAINS_WEB_DIST")]
    web_dist: Option<std::path::PathBuf>,
    /// Extra allowed origins for cookie flows; default is same-origin only.
    #[arg(long, env = "BRAINS_ALLOWED_ORIGINS", value_delimiter = ',')]
    origin: Vec<String>,
    /// Stop advertising these capabilities (comma-separated: only
    /// `document_conversion` and `semantic` can be withheld honestly).
    #[arg(long, env = "BRAINS_DISABLE", value_delimiter = ',')]
    disable: Vec<String>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "brains_server=info,tower_http=warn".into()),
        )
        .init();
    let args = Args::parse();
    let token = match root_token(args.token, args.token_file) {
        Ok(token) => token,
        Err(message) => {
            eprintln!("brains-server: {message}");
            std::process::exit(2);
        }
    };
    let config = ServerConfig {
        data_dir: args.data_dir,
        vault: args.vault,
        bind: args.bind,
        port: args.port,
        token,
        web_dist: args.web_dist,
        allowed_origins: args.origin,
        seed_templates: !args.no_seed,
        disable: args.disable,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let result = runtime.block_on(async {
        #[cfg(unix)]
        let shutdown = {
            use tokio::signal::unix::{SignalKind, signal};
            let mut terminate = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
            let mut interrupt = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
            async move {
                tokio::select! {
                    _ = terminate.recv() => {},
                    _ = interrupt.recv() => {},
                }
            }
        };
        #[cfg(not(unix))]
        let shutdown = async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(%error, "cannot listen for shutdown; stopping server");
            }
        };
        let server = brains_server::serve(config).await?;
        tracing::info!(version = brains_build_info::VERSION, "server started");
        server
            .with_graceful_shutdown(async {
                shutdown.await;
                tracing::info!("shutdown requested; finishing active requests");
            })
            .await
            .map_err(|error| error.to_string())
    });
    if let Err(message) = result {
        eprintln!("brains-server: {message}");
        std::process::exit(2);
    }
}

fn root_token(
    token: Option<String>,
    file: Option<std::path::PathBuf>,
) -> Result<Option<String>, String> {
    let token = match file {
        Some(path) => Some(
            std::fs::read_to_string(&path)
                .map_err(|error| format!("cannot read token file {}: {error}", path.display()))?
                .trim_end_matches(['\r', '\n'])
                .to_owned(),
        ),
        None => token,
    };
    if token
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.chars().any(char::is_whitespace))
    {
        return Err("root token must be nonempty and contain no whitespace".into());
    }
    Ok(token)
}
