//! Command-line contract and dispatch for the `bsx` binary.

use crate::commands;
use crate::config::Config;
use brains_client::TransferKind;
use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bsx", version = env!("BSX_VERSION"), about = "Brains local and remote library manager")]
struct Cli {
    /// Named profile from ~/.brains/config.json (default: active profile).
    #[arg(long, global = true)]
    profile: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args)]
pub(crate) struct LsArgs {
    #[arg(default_value = "")]
    pub(crate) prefix: String,
    #[arg(long)]
    pub(crate) json: bool,
    #[arg(long)]
    pub(crate) deleted: bool,
    #[arg(long, default_value = "store")]
    pub(crate) source: String,
    #[arg(long)]
    pub(crate) tag: Vec<String>,
    /// note | text | script | binary (source=index only)
    #[arg(long)]
    pub(crate) kind: Option<String>,
    #[arg(long, default_value = "path")]
    pub(crate) sort: String,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize an empty local library and make its profile active. No server required.
    Init {
        directory: std::path::PathBuf,
        #[arg(long, default_value = "default")]
        vault: String,
    },
    /// Print a shell completion script without loading profiles or connecting.
    Completion {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
    /// Show supported features and limits of the target instance.
    Capabilities {
        #[arg(long)]
        json: bool,
    },
    /// Print the authenticated principal.
    Whoami {
        #[arg(long)]
        json: bool,
    },
    /// Manage connection profiles.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Store a token for the active profile (read from stdin).
    Login {
        #[arg(long)]
        stdin_token: bool,
    },
    /// List files.
    Ls(LsArgs),
    /// Print a file's current bytes to stdout.
    Cat { path: String },
    /// Print a file's metadata.
    Stat {
        path: String,
        #[arg(long)]
        json: bool,
    },
    /// Create or conditionally update a file. `--create` or `--if-revision`
    /// is mandatory; the CLI never overwrites based on a silent fresh read.
    Put {
        path: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        create: bool,
        #[arg(long)]
        if_revision: Option<String>,
        #[arg(long)]
        content_type: Option<String>,
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Soft-delete a file (tombstone revision).
    Rm {
        path: String,
        #[arg(long)]
        if_revision: String,
    },
    /// Walk the reachable history of a path.
    History {
        path: String,
        #[arg(long)]
        json: bool,
    },
    /// Read one immutable revision by ID.
    Show {
        path: String,
        #[arg(long)]
        revision: String,
        #[arg(long)]
        json: bool,
    },
    /// Restore prior content as a new revision.
    Restore {
        path: String,
        #[arg(long)]
        revision: String,
        #[arg(long)]
        if_revision: String,
    },
    /// Literal or FTS search; pages until complete unless limited.
    Search {
        query: String,
        #[arg(long, default_value = "literal")]
        mode: String,
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        json: bool,
        /// Stop after this many pages even if incomplete (exit code 7).
        #[arg(long)]
        max_pages: Option<usize>,
    },
    /// Outbound links or backlinks of a path.
    Links {
        path: String,
        #[arg(long)]
        backlinks: bool,
        #[arg(long)]
        json: bool,
    },
    /// Exact-text edit (P1).
    Patch {
        path: String,
        #[arg(long)]
        if_revision: String,
        #[arg(long)]
        old: String,
        #[arg(long)]
        new: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Copy a file (P1).
    Cp {
        source: String,
        target: String,
        #[arg(long)]
        if_revision: String,
    },
    /// Move a file (P1): copy create-only, then conditionally trash.
    Mv {
        source: String,
        target: String,
        #[arg(long)]
        if_revision: String,
    },
    /// Unified diff between two revisions (P1).
    Diff {
        path: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
    /// Drive paged index rebuild.
    Reindex {
        #[arg(long)]
        all_pages: bool,
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// Push the built-in template notes into the target vault (existing files
    /// are skipped). Useful for cloud instances deployed without content.
    Seed,
    /// Export the current working tree to a new directory.
    Export {
        output: std::path::PathBuf,
        #[arg(long, value_enum, default_value_t = TransferFormat::WorkingTree)]
        format: TransferFormat,
    },
    /// Import a directory; differing live files require explicit --overwrite.
    Import {
        input: std::path::PathBuf,
        #[arg(long, value_enum, default_value_t = TransferFormat::WorkingTree)]
        format: TransferFormat,
        #[arg(long)]
        overwrite: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Synchronize current files between this profile and another profile.
    Sync {
        /// The other named profile. Pair sync requires profiles stored in config.
        #[arg(long, global = true)]
        remote_profile: Option<String>,
        /// Show planned actions without writing files or sync state.
        #[arg(long)]
        dry_run: bool,
        #[command(subcommand)]
        action: Option<SyncCommand>,
    },
    /// Read a file into $EDITOR and conditionally write it back.
    Edit {
        path: String,
        #[arg(long)]
        if_revision: String,
    },
}

#[derive(Clone, Subcommand)]
pub(crate) enum SyncCommand {
    /// Compare actual bytes and metadata without writing or loading sync state.
    Verify,
    /// Resolve a reported path conflict and write the chosen result to both profiles.
    Resolve {
        path: String,
        /// Keep the selected profile's current state.
        #[arg(long, value_enum, conflicts_with = "merged_file")]
        keep: Option<SyncSide>,
        /// Use a locally edited file as the resolved content.
        #[arg(long, conflicts_with = "keep")]
        merged_file: Option<String>,
        /// Override the merged file's media type.
        #[arg(long, requires = "merged_file")]
        content_type: Option<String>,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum SyncSide {
    Local,
    Remote,
}

#[derive(Subcommand)]
pub(crate) enum ProfileCommand {
    Add {
        name: String,
        #[arg(
            long,
            required_unless_present = "data_dir",
            conflicts_with = "data_dir"
        )]
        url: Option<String>,
        #[arg(long, requires = "url")]
        token: Option<String>,
        /// Attach an existing local library (use init to create one).
        #[arg(long, required_unless_present = "url", conflicts_with = "url")]
        data_dir: Option<std::path::PathBuf>,
        #[arg(long, requires = "data_dir")]
        vault: Option<String>,
    },
    List,
    Use {
        name: String,
        /// Verify against this source and rebuild the destination index before switching.
        #[arg(long)]
        verify_with: Option<String>,
    },
}

pub(crate) fn run() -> i32 {
    let cli = Cli::parse();
    if let Command::Completion { shell } = cli.command {
        let mut command = Cli::command();
        let mut output = std::io::stdout();
        match shell {
            CompletionShell::Nushell => clap_complete::generate(
                clap_complete_nushell::Nushell,
                &mut command,
                "bsx",
                &mut output,
            ),
            other => {
                let shell = match other {
                    CompletionShell::Bash => clap_complete::Shell::Bash,
                    CompletionShell::Zsh => clap_complete::Shell::Zsh,
                    CompletionShell::Fish => clap_complete::Shell::Fish,
                    CompletionShell::Powershell => clap_complete::Shell::PowerShell,
                    CompletionShell::Nushell => unreachable!(),
                };
                clap_complete::generate(shell, &mut command, "bsx", &mut output);
            }
        }
        return 0;
    }
    let mut config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("bsx: {error}");
            return 2;
        }
    };
    if let Command::Init { directory, vault } = cli.command {
        return match commands::system::init(&mut config, cli.profile.as_deref(), &directory, vault)
        {
            Ok(code) => code,
            Err(e) => {
                eprintln!("bsx: {e}");
                e.exit_code()
            }
        };
    }
    // Profile management owns the config file itself and must work before any
    // connection exists — the first-use journey is `bsx profile add` (§12).
    if let Command::Profile { command } = cli.command {
        return match commands::system::profile_command(&mut config, command) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("bsx: {error}");
                error.exit_code()
            }
        };
    }
    if let Command::Sync {
        remote_profile,
        dry_run,
        action,
    } = &cli.command
    {
        return commands::sync::run(
            &config,
            cli.profile.as_deref(),
            remote_profile.as_deref(),
            *dry_run,
            action.clone(),
        );
    }
    let connection = match config.resolve(cli.profile.as_deref()) {
        Ok(connection) => connection,
        Err(message) => {
            eprintln!("bsx: {message}");
            return 2;
        }
    };
    if let Command::Login { stdin_token } = cli.command {
        let crate::config::Profile::Remote { url, .. } = &connection else {
            eprintln!("bsx: local profiles use filesystem access, not tokens");
            return 2;
        };
        return match commands::system::login(&mut config, cli.profile.as_deref(), url, stdin_token)
        {
            Ok(code) => code,
            Err(e) => {
                eprintln!("bsx: {e}");
                e.exit_code()
            }
        };
    }
    let client = match connection.connect() {
        Ok(client) => client,
        Err(e) => {
            eprintln!("bsx: {e}");
            return e.exit_code();
        }
    };
    let client = client.as_ref();

    let result = match cli.command {
        Command::Capabilities { json } => commands::system::capabilities(client, json),
        Command::Whoami { json } => commands::system::whoami(client, json),
        Command::Profile { command } => commands::system::profile_command(&mut config, command),
        Command::Login { .. } | Command::Init { .. } => {
            unreachable!("handled before opening the library")
        }
        Command::Ls(args) => commands::query::ls(client, args),
        Command::Cat { path } => commands::files::cat(client, &path),
        Command::Stat { path, json } => commands::files::stat(client, &path, json),
        Command::Put {
            path,
            from,
            create,
            if_revision,
            content_type,
            operation_id,
        } => commands::files::put(
            client,
            &path,
            from,
            create,
            if_revision,
            content_type,
            operation_id,
        ),
        Command::Rm { path, if_revision } => commands::files::rm(client, &path, &if_revision),
        Command::History { path, json } => commands::files::history(client, &path, json),
        Command::Show {
            path,
            revision,
            json,
        } => commands::files::show(client, &path, &revision, json),
        Command::Restore {
            path,
            revision,
            if_revision,
        } => commands::files::restore(client, &path, &revision, &if_revision),
        Command::Search {
            query,
            mode,
            prefix,
            tag,
            json,
            max_pages,
        } => commands::query::search(client, &query, &mode, prefix, tag, json, max_pages),
        Command::Links {
            path,
            backlinks,
            json,
        } => commands::query::links(client, &path, backlinks, json),
        Command::Patch {
            path,
            if_revision,
            old,
            new,
            dry_run,
        } => commands::files::patch(client, &path, &if_revision, &old, &new, dry_run),
        Command::Cp {
            source,
            target,
            if_revision,
        } => commands::files::transfer(client, TransferKind::Copy, &source, &target, &if_revision),
        Command::Mv {
            source,
            target,
            if_revision,
        } => commands::files::transfer(client, TransferKind::Move, &source, &target, &if_revision),
        Command::Diff { path, from, to } => commands::files::diff(client, &path, &from, &to),
        Command::Reindex { all_pages, limit } => {
            commands::system::reindex(client, all_pages, limit)
        }
        Command::Seed => commands::system::seed_template(client),
        Command::Export { output, format } => commands::system::export(client, &output, format),
        Command::Import {
            input,
            format,
            overwrite,
            dry_run,
        } => commands::system::import(client, &input, format, overwrite, dry_run),
        Command::Sync { .. } => unreachable!("sync is handled before single-profile resolution"),
        Command::Completion { .. } => unreachable!("completion is handled before config loading"),
        Command::Edit { path, if_revision } => commands::files::edit(client, &path, &if_revision),
    };

    match result {
        Ok(code) => code,
        Err(error) => {
            if let Some(current) = &error.current_revision {
                eprintln!("bsx: {error} (current revision: {current})");
            } else {
                eprintln!("bsx: {error}");
            }
            error.exit_code()
        }
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
    #[value(alias = "nu")]
    Nushell,
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum TransferFormat {
    WorkingTree,
    Archive,
}
