//! Command-line contract and dispatch for the `bsx` binary.

use crate::commands;
use crate::config::Config;
use brains_client::{BrainClient, TransferKind};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "brain", version, about = "Brains command line client")]
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
    pub(crate) tag: Option<String>,
    /// note | text | script | binary (source=index only)
    #[arg(long)]
    pub(crate) kind: Option<String>,
    #[arg(long, default_value = "path")]
    pub(crate) sort: String,
}

#[derive(Subcommand)]
enum Command {
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
        tag: Option<String>,
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
    Export { output: String },
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
        #[arg(long)]
        url: String,
        #[arg(long)]
        token: Option<String>,
    },
    List,
    Use {
        name: String,
    },
}

pub(crate) fn run() -> i32 {
    let cli = Cli::parse();
    let mut config = Config::load();
    // Profile management owns the config file itself and must work before any
    // connection exists — the first-use journey is `brain profile add` (§12).
    if let Command::Profile { command } = cli.command {
        return match commands::system::profile_command(&mut config, command) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("brain: {error}");
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
            eprintln!("brain: {message}");
            return 2;
        }
    };
    let client = BrainClient::new(&connection.0, &connection.1);

    let result = match cli.command {
        Command::Capabilities { json } => commands::system::capabilities(&client, json),
        Command::Whoami { json } => commands::system::whoami(&client, json),
        Command::Profile { command } => commands::system::profile_command(&mut config, command),
        Command::Login { stdin_token } => {
            commands::system::login(&mut config, &connection.0, stdin_token)
        }
        Command::Ls(args) => commands::query::ls(&client, args),
        Command::Cat { path } => commands::files::cat(&client, &path),
        Command::Stat { path, json } => commands::files::stat(&client, &path, json),
        Command::Put {
            path,
            from,
            create,
            if_revision,
            content_type,
            operation_id,
        } => commands::files::put(
            &client,
            &path,
            from,
            create,
            if_revision,
            content_type,
            operation_id,
        ),
        Command::Rm { path, if_revision } => commands::files::rm(&client, &path, &if_revision),
        Command::History { path, json } => commands::files::history(&client, &path, json),
        Command::Show {
            path,
            revision,
            json,
        } => commands::files::show(&client, &path, &revision, json),
        Command::Restore {
            path,
            revision,
            if_revision,
        } => commands::files::restore(&client, &path, &revision, &if_revision),
        Command::Search {
            query,
            mode,
            prefix,
            tag,
            json,
            max_pages,
        } => commands::query::search(&client, &query, &mode, prefix, tag, json, max_pages),
        Command::Links {
            path,
            backlinks,
            json,
        } => commands::query::links(&client, &path, backlinks, json),
        Command::Patch {
            path,
            if_revision,
            old,
            new,
            dry_run,
        } => commands::files::patch(&client, &path, &if_revision, &old, &new, dry_run),
        Command::Cp {
            source,
            target,
            if_revision,
        } => commands::files::transfer(&client, TransferKind::Copy, &source, &target, &if_revision),
        Command::Mv {
            source,
            target,
            if_revision,
        } => commands::files::transfer(&client, TransferKind::Move, &source, &target, &if_revision),
        Command::Diff { path, from, to } => commands::files::diff(&client, &path, &from, &to),
        Command::Reindex { all_pages, limit } => {
            commands::system::reindex(&client, all_pages, limit)
        }
        Command::Seed => commands::system::seed_template(&client),
        Command::Export { output } => commands::system::export(&client, &output),
        Command::Sync { .. } => unreachable!("sync is handled before single-profile resolution"),
        Command::Edit { path, if_revision } => commands::files::edit(&client, &path, &if_revision),
    };

    match result {
        Ok(code) => code,
        Err(error) => {
            if let Some(current) = &error.current_revision {
                eprintln!("brain: {error} (current revision: {current})");
            } else {
                eprintln!("brain: {error}");
            }
            error.exit_code()
        }
    }
}
