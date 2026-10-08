//! `brain` — the BrainStore CLI (design §12). Every remote mutation carries
//! an explicit revision precondition; structured commands support `--json`;
//! stdout stays clean for pipes (progress and errors go to stderr).

mod config;

use clap::{Parser, Subcommand};
use config::{Config, Profile};

use brainstore_client::{
    BrainClient, ClientError, ClientResult, PatchOutcomeClient, Precondition, TransferKind,
    urlencode,
};

#[derive(Parser)]
#[command(name = "brain", version, about = "BrainStore command line client")]
struct Cli {
    /// Named profile from ~/.brainstore/config.json (default: active profile).
    #[arg(long, global = true)]
    profile: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args)]
struct LsArgs {
    #[arg(default_value = "")]
    prefix: String,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    deleted: bool,
    #[arg(long, default_value = "store")]
    source: String,
    #[arg(long)]
    tag: Option<String>,
    /// note | text | script | binary (source=index only)
    #[arg(long)]
    kind: Option<String>,
    #[arg(long, default_value = "path")]
    sort: String,
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
    /// Read a file into $EDITOR and conditionally write it back.
    Edit {
        path: String,
        #[arg(long)]
        if_revision: String,
    },
}

#[derive(Subcommand)]
enum ProfileCommand {
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

fn main() {
    let cli = Cli::parse();
    let code = run(cli);
    std::process::exit(code);
}

fn run(cli: Cli) -> i32 {
    let mut config = Config::load();
    // Profile management owns the config file itself and must work before any
    // connection exists — the first-use journey is `brain profile add` (§12).
    if let Command::Profile { command } = cli.command {
        return match profile_command(&mut config, command) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("brain: {error}");
                error.exit_code()
            }
        };
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
        Command::Capabilities { json } => capabilities(&client, json),
        Command::Whoami { json } => whoami(&client, json),
        Command::Profile { command } => profile_command(&mut config, command),
        Command::Login { stdin_token } => login(&mut config, &connection.0, stdin_token),
        Command::Ls(args) => ls(&client, args),
        Command::Cat { path } => cat(&client, &path),
        Command::Stat { path, json } => stat(&client, &path, json),
        Command::Put {
            path,
            from,
            create,
            if_revision,
            content_type,
            operation_id,
        } => put(
            &client,
            &path,
            from,
            create,
            if_revision,
            content_type,
            operation_id,
        ),
        Command::Rm { path, if_revision } => rm(&client, &path, &if_revision),
        Command::History { path, json } => history(&client, &path, json),
        Command::Show {
            path,
            revision,
            json,
        } => show(&client, &path, &revision, json),
        Command::Restore {
            path,
            revision,
            if_revision,
        } => restore(&client, &path, &revision, &if_revision),
        Command::Search {
            query,
            mode,
            prefix,
            tag,
            json,
            max_pages,
        } => search(&client, &query, &mode, prefix, tag, json, max_pages),
        Command::Links {
            path,
            backlinks,
            json,
        } => links(&client, &path, backlinks, json),
        Command::Patch {
            path,
            if_revision,
            old,
            new,
            dry_run,
        } => patch(&client, &path, &if_revision, &old, &new, dry_run),
        Command::Cp {
            source,
            target,
            if_revision,
        } => transfer(&client, TransferKind::Copy, &source, &target, &if_revision),
        Command::Mv {
            source,
            target,
            if_revision,
        } => transfer(&client, TransferKind::Move, &source, &target, &if_revision),
        Command::Diff { path, from, to } => diff(&client, &path, &from, &to),
        Command::Reindex { all_pages, limit } => reindex(&client, all_pages, limit),
        Command::Seed => seed_template(&client),
        Command::Export { output } => export(&client, &output),
        Command::Edit { path, if_revision } => edit(&client, &path, &if_revision),
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

fn print_pretty<T: serde::Serialize>(value: &T) -> ClientResult<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| ClientError::transport(e.to_string()))?
    );
    Ok(())
}

fn capabilities(client: &BrainClient, json: bool) -> ClientResult<i32> {
    let caps = client.capabilities()?;
    if json {
        print_pretty(&caps)?;
    } else {
        println!("api_version   {}", caps.api_version);
        println!("store_format  {}", caps.store_format);
        println!(
            "runtime       {}",
            serde_json::to_value(caps.runtime).unwrap_or_default()
        );
        if let Some(vault) = &caps.vault {
            println!("vault         {vault}");
        }
        println!(
            "limits        text<={}B attachments<={}B",
            caps.limits.max_text_bytes, caps.limits.max_attachment_bytes
        );
        for (feature, enabled) in &caps.features {
            println!("feature       {feature}={enabled}");
        }
    }
    Ok(0)
}

fn whoami(client: &BrainClient, json: bool) -> ClientResult<i32> {
    let session = client.session()?;
    if json {
        print_pretty(&session)?;
    } else {
        println!("actor   {}", session.actor);
        println!("scopes  {}", session.scopes.join(", "));
    }
    Ok(0)
}

fn profile_command(config: &mut Config, command: ProfileCommand) -> ClientResult<i32> {
    match command {
        ProfileCommand::Add { name, url, token } => {
            config.profiles.insert(
                name.clone(),
                Profile {
                    url,
                    token: token.unwrap_or_default(),
                },
            );
            if config.active.is_none() {
                config.active = Some(name);
            }
            config.save().map_err(ClientError::transport)?;
            eprintln!("profile saved");
            Ok(0)
        }
        ProfileCommand::List => {
            for (name, profile) in &config.profiles {
                let marker = if config.active.as_deref() == Some(name.as_str()) {
                    "*"
                } else {
                    " "
                };
                println!("{marker} {name}  {}", profile.url);
            }
            Ok(0)
        }
        ProfileCommand::Use { name } => {
            if !config.profiles.contains_key(&name) {
                return Err(ClientError {
                    status: 400,
                    code: Some("INVALID_ARGUMENT".into()),
                    message: format!("no profile `{name}`"),
                    current_revision: None,
                });
            }
            config.active = Some(name);
            config.save().map_err(ClientError::transport)?;
            Ok(0)
        }
    }
}

fn login(config: &mut Config, url: &str, stdin_token: bool) -> ClientResult<i32> {
    if !stdin_token {
        return Err(ClientError {
            status: 400,
            code: Some("INVALID_ARGUMENT".into()),
            message: "pass --stdin-token; the CLI never reads tokens from argv history".into(),
            current_revision: None,
        });
    }
    let mut token = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut token)
        .map_err(|e| ClientError::transport(e.to_string()))?;
    let token = token.trim().to_owned();
    let name = config.active.clone().unwrap_or_else(|| "default".into());
    let entry = config.profiles.entry(name).or_insert_with(|| Profile {
        url: url.to_owned(),
        token: String::new(),
    });
    entry.token = token;
    config.save().map_err(ClientError::transport)?;
    eprintln!("token stored");
    Ok(0)
}

fn ls(client: &BrainClient, args: LsArgs) -> ClientResult<i32> {
    let LsArgs {
        mut prefix,
        json,
        deleted,
        source,
        tag,
        kind,
        sort,
    } = args;
    // `brain ls /` reads as "the whole vault" (§6.1): paths are vault-relative
    // and never start with a slash.
    if prefix == "/" {
        prefix.clear();
    }
    let mut cursor: Option<String> = None;
    loop {
        let mut query = format!("source={source}&limit=50");
        if !prefix.is_empty() {
            query.push_str(&format!("&prefix={}", urlencode(&prefix)));
        }
        if deleted {
            query.push_str("&include_deleted=true");
        }
        if let Some(tag) = &tag {
            query.push_str(&format!("&tag={}", urlencode(tag)));
        }
        if let Some(kind) = &kind {
            query.push_str(&format!("&kind={kind}"));
        }
        if sort != "path" {
            query.push_str(&format!("&sort={sort}"));
        }
        if let Some(cursor) = &cursor {
            query.push_str(&format!("&cursor={}", urlencode(cursor)));
        }
        let page = client.list_files(&query)?;
        for entry in &page.items {
            if json {
                println!(
                    "{}",
                    serde_json::to_string(entry)
                        .map_err(|e| ClientError::transport(e.to_string()))?
                );
            } else {
                let state = if entry.state == "trashed" {
                    "[trashed]"
                } else {
                    ""
                };
                let title = entry.title.clone().unwrap_or_default();
                println!("{}\t{}\t{state}{title}", entry.path, entry.revision);
            }
        }
        match page.next_cursor.clone() {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(0)
}

fn cat(client: &BrainClient, path: &str) -> ClientResult<i32> {
    let (bytes, _etag) = client.get_raw(path)?;
    use std::io::Write;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|e| ClientError::transport(e.to_string()))?;
    Ok(0)
}

fn stat(client: &BrainClient, path: &str, json: bool) -> ClientResult<i32> {
    let entry = client.get_meta(path)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&entry)
                .map_err(|e| ClientError::transport(e.to_string()))?
        );
    } else {
        println!("path        {}", entry.path);
        println!("revision    {}", entry.revision);
        println!("generation  {}", entry.generation);
        println!("state       {}", entry.state);
        if let Some(size) = entry.size {
            println!("size        {size}");
        }
        if let Some(kind) = &entry.kind {
            println!(
                "kind        {}",
                serde_json::to_value(kind).unwrap_or_default()
            );
        }
        if let Some(updated) = &entry.updated_at {
            println!("updated_at  {updated}");
        }
    }
    Ok(0)
}

fn put(
    client: &BrainClient,
    path: &str,
    from: Option<String>,
    create: bool,
    if_revision: Option<String>,
    content_type: Option<String>,
    operation_id: Option<String>,
) -> ClientResult<i32> {
    let precondition = match (create, if_revision) {
        (true, None) => Precondition::Create,
        (false, Some(rev)) => Precondition::IfRevision(rev),
        (true, Some(_)) => {
            return Err(ClientError {
                status: 400,
                code: Some("INVALID_ARGUMENT".into()),
                message: "use either --create or --if-revision, not both".into(),
                current_revision: None,
            });
        }
        (false, None) => {
            return Err(ClientError {
                status: 428,
                code: Some("PRECONDITION_REQUIRED".into()),
                message: "pass --create or --if-revision <REV>; refusing an unconditional write"
                    .into(),
                current_revision: None,
            });
        }
    };
    let bytes = match &from {
        Some(file) => {
            std::fs::read(file).map_err(|e| ClientError::transport(format!("read {file}: {e}")))?
        }
        None => {
            use std::io::Read;
            let mut buffer = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buffer)
                .map_err(|e| ClientError::transport(e.to_string()))?;
            buffer
        }
    };
    let media = content_type.unwrap_or_else(|| brainstore_domain::media_type_for(path).to_owned());
    let result = client.put_content(path, &bytes, &media, precondition, operation_id.as_deref())?;
    println!(
        "{} gen={} rev={}",
        if result.entry.generation == 1 {
            "created"
        } else {
            "updated"
        },
        result.entry.generation,
        result.entry.revision
    );
    Ok(0)
}

fn rm(client: &BrainClient, path: &str, if_revision: &str) -> ClientResult<i32> {
    let result = client.delete(path, if_revision, None)?;
    println!(
        "trashed gen={} rev={}",
        result.entry.generation, result.entry.revision
    );
    Ok(0)
}

fn history(client: &BrainClient, path: &str, json: bool) -> ClientResult<i32> {
    let mut cursor: Option<String> = None;
    loop {
        let page = client.history(path, cursor.as_deref(), 20)?;
        for revision in &page.items {
            if json {
                println!(
                    "{}",
                    serde_json::to_string(revision)
                        .map_err(|e| ClientError::transport(e.to_string()))?
                );
            } else {
                println!(
                    "gen {:>4}  {}  {:8}  {}  {}",
                    revision.generation,
                    revision.created_at,
                    revision.action,
                    revision.revision,
                    revision.parent.as_deref().unwrap_or("-")
                );
            }
        }
        match page.next_cursor.clone() {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(0)
}

fn show(client: &BrainClient, path: &str, revision: &str, json: bool) -> ClientResult<i32> {
    if json {
        print_pretty(&client.revision_meta(path, revision)?)?;
        return Ok(0);
    }
    let bytes = client.revision_raw(path, revision)?;
    use std::io::Write;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|e| ClientError::transport(e.to_string()))?;
    Ok(0)
}

fn restore(
    client: &BrainClient,
    path: &str,
    revision: &str,
    if_current: &str,
) -> ClientResult<i32> {
    let result = client.restore(path, revision, if_current, None)?;
    println!(
        "restored gen={} rev={}",
        result.entry.generation, result.entry.revision
    );
    Ok(0)
}

fn search(
    client: &BrainClient,
    query: &str,
    mode: &str,
    prefix: Option<String>,
    tag: Option<String>,
    json: bool,
    max_pages: Option<usize>,
) -> ClientResult<i32> {
    let mut cursor: Option<String> = None;
    let mut pages = 0usize;
    let mut total_hits = 0usize;
    let mut complete;
    loop {
        let mut q = format!("q={}&mode={mode}", urlencode(query));
        if let Some(prefix) = &prefix {
            q.push_str(&format!("&prefix={}", urlencode(prefix)));
        }
        if let Some(tag) = &tag {
            q.push_str(&format!("&tag={}", urlencode(tag)));
        }
        if let Some(cursor) = &cursor {
            q.push_str(&format!("&cursor={}", urlencode(cursor)));
        }
        let page = client.search(&q)?;
        pages += 1;
        total_hits += page.items.len();
        for hit in &page.items {
            if json {
                println!(
                    "{}",
                    serde_json::to_string(hit)
                        .map_err(|e| ClientError::transport(e.to_string()))?
                );
            } else {
                let snippet: String = hit.snippet.chars().take(120).collect();
                println!("{}\t{}", hit.path, snippet.replace('\n', " "));
            }
        }
        complete = page.complete;
        match page.next_cursor.clone() {
            Some(next) if max_pages.is_none_or(|max| pages < max) => cursor = Some(next),
            _ => break,
        }
    }
    if !complete {
        eprintln!("brain: search incomplete after {pages} pages; results may be partial");
        return Ok(7);
    }
    Ok(if total_hits == 0 { 1 } else { 0 })
}

fn links(client: &BrainClient, path: &str, backlinks: bool, json: bool) -> ClientResult<i32> {
    let mut cursor: Option<String> = None;
    loop {
        let page = client.links(path, backlinks, cursor.as_deref())?;
        for item in &page.items {
            if json {
                println!(
                    "{}",
                    serde_json::to_string(item)
                        .map_err(|e| ClientError::transport(e.to_string()))?
                );
            } else if backlinks {
                println!("{}\t{}", item.source, item.kind);
            } else {
                println!("{}\t{}", item.target, item.kind);
            }
        }
        match page.next_cursor.clone() {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(0)
}

fn patch(
    client: &BrainClient,
    path: &str,
    if_revision: &str,
    old: &str,
    new: &str,
    dry_run: bool,
) -> ClientResult<i32> {
    let body = brainstore_api_types::PatchBody {
        edits: vec![brainstore_api_types::PatchEditDto {
            old_text: old.to_owned(),
            new_text: new.to_owned(),
            expected_matches: 1,
        }],
        dry_run,
    };
    match client.patch(path, if_revision, body, None)? {
        PatchOutcomeClient::DryRun(dry) => {
            print!("{}", dry.diff);
            Ok(0)
        }
        PatchOutcomeClient::Committed(result) => {
            println!(
                "patched gen={} rev={}",
                result.entry.generation, result.entry.revision
            );
            Ok(0)
        }
    }
}

fn transfer(
    client: &BrainClient,
    kind: TransferKind,
    source: &str,
    target: &str,
    if_revision: &str,
) -> ClientResult<i32> {
    let operation_id = brainstore_domain::HexId::generate().as_str().to_owned();
    let result = client.transfer(
        kind,
        brainstore_api_types::TransferBody {
            source: source.to_owned(),
            target: target.to_owned(),
            expected_source_revision: if_revision.to_owned(),
            operation_id,
        },
    )?;
    let verb = match kind {
        TransferKind::Copy => "copied",
        TransferKind::Move => "moved",
    };
    if result.partial {
        eprintln!(
            "brain: partial move: target created, source NOT trashed; repair manually (operation {})",
            result.operation_id
        );
        return Ok(7);
    }
    println!(
        "{verb} target_gen_rev={}{}",
        result.target_revision.as_deref().unwrap_or("-"),
        if result.source_trashed {
            " source_trashed"
        } else {
            ""
        }
    );
    Ok(0)
}

fn diff(client: &BrainClient, path: &str, from: &str, to: &str) -> ClientResult<i32> {
    let result = client.diff(path, from, to)?;
    print!("{}", result.diff);
    Ok(0)
}

fn reindex(client: &BrainClient, all_pages: bool, limit: usize) -> ClientResult<i32> {
    let mut cursor: Option<String> = None;
    let mut processed = 0i64;
    let mut failed = 0usize;
    loop {
        let page = client.reindex(cursor.as_deref(), limit)?;
        processed += page.processed;
        failed += page.failed.len();
        for failure in &page.failed {
            eprintln!("brain: reindex failed {}: {}", failure.path, failure.code);
        }
        if let Some(next) = page.next_cursor.clone() {
            if all_pages {
                cursor = Some(next);
                continue;
            }
            eprintln!("brain: more pages pending; rerun with --all-pages");
            return Ok(7);
        }
        break;
    }
    println!("reindexed {processed} files, {failed} failures");
    Ok(0)
}

fn export(client: &BrainClient, output: &str) -> ClientResult<i32> {
    let output = std::path::Path::new(output);
    let vault = client
        .capabilities()?
        .vault
        .unwrap_or_else(|| "default".into());
    let summary = brainstore_client::export::export_working_tree(client, output, &vault)?;
    println!("{summary}");
    Ok(0)
}

/// Push the built-in template notes into the target vault: existing paths are
/// skipped, missing ones are created. Idempotent — rerun anytime.
fn seed_template(client: &BrainClient) -> ClientResult<i32> {
    let mut written = 0usize;
    let mut skipped = 0usize;
    for (path, body) in brainstore_application::seed::template_notes() {
        match client.get_meta(path) {
            Ok(_) => skipped += 1,
            Err(error) if error.status == 404 => {
                client.put_content(
                    path,
                    body.as_bytes(),
                    "text/markdown",
                    Precondition::Create,
                    None,
                )?;
                written += 1;
                println!("seeded {path}");
            }
            Err(error) => return Err(error),
        }
    }
    println!("done: {written} seeded, {skipped} already present.");
    Ok(0)
}

fn edit(client: &BrainClient, path: &str, if_revision: &str) -> ClientResult<i32> {
    let (bytes, _etag) = client.get_raw(path)?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".into());
    let temp = std::env::temp_dir().join(format!(
        "brain-edit-{}",
        brainstore_domain::HexId::generate()
    ));
    std::fs::write(&temp, &bytes).map_err(|e| ClientError::transport(e.to_string()))?;
    let status = std::process::Command::new(&editor)
        .arg(&temp)
        .status()
        .map_err(|e| ClientError::transport(format!("launch {editor}: {e}")))?;
    if !status.success() {
        eprintln!(
            "brain: editor exited with {status}; edited bytes kept at {}",
            temp.display()
        );
        return Ok(8);
    }
    let edited = std::fs::read(&temp).map_err(|e| ClientError::transport(e.to_string()))?;
    if edited == bytes {
        eprintln!("brain: no changes");
        let _ = std::fs::remove_file(&temp);
        return Ok(0);
    }
    let media = brainstore_domain::media_type_for(path).to_owned();
    match client.put_content(
        path,
        &edited,
        &media,
        Precondition::IfRevision(if_revision.to_owned()),
        None,
    ) {
        Ok(result) => {
            let _ = std::fs::remove_file(&temp);
            println!(
                "saved gen={} rev={}",
                result.entry.generation, result.entry.revision
            );
            Ok(0)
        }
        Err(error) => {
            // Keep the edited bytes locally on conflict; never auto-retry.
            eprintln!(
                "brain: save failed ({}); edited bytes kept at {}",
                error,
                temp.display()
            );
            Err(error)
        }
    }
}
