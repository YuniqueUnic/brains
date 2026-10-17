//! Command-line contract and dispatch for the `bsx` binary.

use crate::commands;
use crate::config::Config;
use brains_client::TransferKind;
use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bsx", version = brains_build_info::VERSION, about = "Brains local and remote library manager")]
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
    /// draft | stable | deprecated (source=index only)
    #[arg(long)]
    pub(crate) status: Option<String>,
    /// Note frontmatter type, exact match (source=index only)
    #[arg(long)]
    pub(crate) type_: Option<String>,
    /// unverified | machine-confirmed | human-reviewed (source=index only)
    #[arg(long)]
    pub(crate) trust: Option<String>,
    /// Only notes past their stale_after date
    #[arg(long, conflicts_with = "fresh")]
    pub(crate) stale: bool,
    /// Only notes not past their stale_after date
    #[arg(long)]
    pub(crate) fresh: bool,
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
        /// draft | stable | deprecated
        #[arg(long)]
        status: Option<String>,
        /// Note frontmatter type, exact match
        #[arg(long)]
        type_: Option<String>,
        /// unverified | machine-confirmed | human-reviewed
        #[arg(long)]
        trust: Option<String>,
        /// Only notes past their stale_after date
        #[arg(long, conflicts_with = "fresh")]
        stale: bool,
        /// Only notes not past their stale_after date
        #[arg(long)]
        fresh: bool,
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
    /// Edit a note by exact text replacement.
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
    /// Copy a file.
    Cp {
        source: String,
        target: String,
        #[arg(long)]
        if_revision: String,
    },
    /// Move a file: copy create-only, then conditionally trash.
    Mv {
        source: String,
        target: String,
        #[arg(long)]
        if_revision: String,
    },
    /// Unified diff between two revisions.
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
    /// Regenerate `<prefix>/index.md` from the current listing (OKF §8), so
    /// hand-written scope indexes stop drifting.
    IndexScope {
        #[arg(default_value = "")]
        prefix: String,
        /// Required when the index note already exists (create-only otherwise).
        #[arg(long)]
        if_revision: Option<String>,
    },
    /// Push the built-in template notes into the target vault (existing files
    /// are skipped). Useful for cloud instances deployed without content.
    Seed {
        /// Overwrite existing template paths whose content differs from the
        /// built-in templates (local edits to those paths are replaced).
        #[arg(long)]
        refresh: bool,
    },
    /// Upgrade bsx (and a sibling brains-server) from GitHub releases.
    Update {
        #[command(flatten)]
        args: commands::update::UpdateArgs,
    },
    /// Export the current working tree to a new directory.
    Export {
        output: std::path::PathBuf,
        #[arg(long, value_enum, default_value_t = TransferFormat::WorkingTree)]
        format: TransferFormat,
        /// OKF: also generate `log.md` from revision dates.
        #[arg(long)]
        with_log: bool,
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
    /// Report stale, unverified and malformed notes (read-only; exit 10 when
    /// findings exist so CI or an agent's end-of-task routine can gate on it).
    Check {
        /// Limit the report to one directory subtree.
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long)]
        json: bool,
        /// OKF v0.2 conformance mode: frontmatter with non-empty `type` on
        /// every non-reserved note, reserved files follow the spec.
        #[arg(long)]
        okf: bool,
    },
    /// Read or update the per-scope checkpoint note (`<scope>/checkpoint.md`,
    /// an ordinary versioned file).
    Checkpoint {
        #[command(subcommand)]
        command: CheckpointCommand,
    },
    /// Import a web page as a Markdown note through the fetch ladder
    /// (markdown negotiation → static HTML → rendered → document), with
    /// strict-by-default network policy.
    ImportUrl {
        url: String,
        path: String,
        /// Prefer the rendered step even when static HTML yields text.
        #[arg(long)]
        render: bool,
        /// Also allow http:// (intranet / local development).
        #[arg(long)]
        allow_http: bool,
        /// Exempt this host or IP from the private-target refusal (repeatable).
        #[arg(long = "allow-host")]
        allow_host: Vec<String>,
        /// Ignore robots.txt for this fetch.
        #[arg(long)]
        ignore_robots: bool,
        #[arg(long, default_value_t = 30)]
        timeout_secs: u64,
        /// Maximum response size in bytes.
        #[arg(long, default_value_t = 10 * 1024 * 1024)]
        max_bytes: usize,
    },
    /// Convert a document attachment into a Markdown companion note
    /// (`<path>.md`) with provenance frontmatter; `status: draft` until
    /// reviewed.
    Convert {
        path: String,
        /// Companion note target (default `<path>.md`).
        #[arg(long)]
        to: Option<String>,
    },
    /// Write current files plus a base-revision manifest into a new directory.
    Checkout {
        directory: std::path::PathBuf,
        /// Limit the copy to one directory subtree.
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Show local and store changes since checkout (needs a working copy).
    Status {
        directory: std::path::PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Update unmodified files from the store; report both-sides conflicts.
    Pull {
        directory: std::path::PathBuf,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
    /// Write local changes with the manifest's CAS preconditions.
    Push {
        directory: std::path::PathBuf,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Subcommand)]
pub(crate) enum CheckpointCommand {
    /// Print the checkpoint body and its current revision.
    Show {
        /// Scope directory; empty means the vault root checkpoint.
        #[arg(default_value = "")]
        scope: String,
    },
    /// Write the checkpoint from a file or stdin with the usual CAS rules.
    Update {
        #[arg(default_value = "")]
        scope: String,
        /// Read the new body from this file (default: stdin).
        #[arg(long)]
        from: Option<String>,
        /// Write only if the current revision matches (from `checkpoint show`).
        #[arg(long)]
        if_revision: Option<String>,
        /// Create the checkpoint only if it does not exist yet.
        #[arg(long, conflicts_with = "if_revision")]
        create: bool,
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
    // Self-upgrade needs no profile, vault or config: it works on a machine
    // that has never run bsx before.
    if let Command::Update { args } = cli.command {
        return match commands::update::update(args) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("bsx: {e}");
                e.exit_code()
            }
        };
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
        Command::Login { .. } | Command::Init { .. } | Command::Update { .. } => {
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
            status,
            type_,
            trust,
            stale,
            fresh,
            json,
            max_pages,
        } => commands::query::search(
            client, &query, &mode, prefix, tag, status, type_, trust, stale, fresh, json, max_pages,
        ),
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
        Command::IndexScope {
            prefix,
            if_revision,
        } => commands::system::index_scope(client, &prefix, if_revision.as_deref()),
        Command::Seed { refresh } => commands::system::seed_template(client, refresh),
        Command::Export {
            output,
            format,
            with_log,
        } => commands::system::export(client, &output, format, with_log),
        Command::Import {
            input,
            format,
            overwrite,
            dry_run,
        } => commands::system::import(client, &input, format, overwrite, dry_run),
        Command::Sync { .. } => unreachable!("sync is handled before single-profile resolution"),
        Command::Completion { .. } => unreachable!("completion is handled before config loading"),
        Command::Edit { path, if_revision } => commands::files::edit(client, &path, &if_revision),
        Command::Check { prefix, json, okf } => {
            commands::check::check(client, prefix.as_deref(), json, okf)
        }
        Command::Checkpoint { command } => match command {
            CheckpointCommand::Show { scope } => commands::checkpoint::show(client, &scope),
            CheckpointCommand::Update {
                scope,
                from,
                if_revision,
                create,
            } => commands::checkpoint::update(
                client,
                &scope,
                from.as_deref(),
                if_revision.as_deref(),
                create,
            ),
        },
        Command::Convert { path, to } => commands::convert::convert(client, &path, to.as_deref()),
        Command::ImportUrl {
            url,
            path,
            render,
            allow_http,
            allow_host,
            ignore_robots,
            timeout_secs,
            max_bytes,
        } => commands::url_import::import_url_command(
            client,
            &url,
            &path,
            render,
            allow_http,
            allow_host,
            ignore_robots,
            timeout_secs,
            max_bytes,
        ),
        Command::Checkout { directory, prefix } => {
            commands::working_copy::checkout(client, &directory, prefix.as_deref())
        }
        Command::Status { directory, json } => {
            commands::working_copy::status(client, &directory, json)
        }
        Command::Pull {
            directory,
            dry_run,
            json,
        } => commands::working_copy::pull(client, &directory, dry_run, json),
        Command::Push {
            directory,
            dry_run,
            json,
        } => commands::working_copy::push(client, &directory, dry_run, json),
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
    /// Open Knowledge Format v0.2 bundle: per-directory generated `index.md`,
    /// optional `log.md`, concept files byte-identical.
    Okf,
}
