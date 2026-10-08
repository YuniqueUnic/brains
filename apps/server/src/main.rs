//! brain-server binary: parse CLI flags and run the library server.

use clap::Parser;

use brains_server::ServerConfig;

#[derive(Parser, Debug)]
#[command(name = "brain-server", about = "Brains local server")]
struct Args {
    /// Durable data directory (store, index db, auth registry, signing keys).
    #[arg(long, default_value = "./brain-data")]
    data_dir: std::path::PathBuf,
    /// Vault name inside the store; one instance manages one vault.
    #[arg(long, default_value = "default")]
    vault: String,
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
    #[arg(long, default_value_t = 8787)]
    port: u16,
    /// Register (or replace) a whole-vault root token instead of generating
    /// one. Also read from BRAINS_TOKEN. Printed once when generated.
    #[arg(long)]
    token: Option<String>,
    /// Serve a built web app (directory with index.html) from the same origin.
    #[arg(long)]
    web_dist: Option<std::path::PathBuf>,
    /// Extra allowed origins for cookie flows; default is same-origin only.
    #[arg(long)]
    origin: Vec<String>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "brains_server=info,tower_http=warn".into()),
        )
        .init();
    let args = Args::parse();
    let config = ServerConfig {
        data_dir: args.data_dir,
        vault: args.vault,
        bind: args.bind,
        port: args.port,
        token: args.token.or_else(|| std::env::var("BRAINS_TOKEN").ok()),
        web_dist: args.web_dist,
        allowed_origins: args.origin,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let code = match runtime.block_on(brains_server::serve(config)) {
        Ok(addr) => {
            // serve() spawns the accept loop; park this thread for process life.
            tracing::info!("serving on {addr}");
            loop {
                std::thread::park();
            }
        }
        Err(message) => {
            eprintln!("brains-server: {message}");
            2
        }
    };
    std::process::exit(code);
}
