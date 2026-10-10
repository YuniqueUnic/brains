//! Native storage assembly shared by the standalone CLI and HTTP server.
//! No network listener, authentication registry, or background runtime.
use brains_adapter_fs::{FsBlobStore, FsHeadStore, FsRevisionStore, FsStore};
use brains_adapter_turso::{INDEX_FILE, OTHER_INDEX_FILE, TursoIndex};
use brains_application::Vault;
use brains_domain::{CursorSigner, DomainError, DomainResult, Limits, Principal, Scope};
use std::path::Path;

pub type LocalVault = Vault<FsHeadStore, FsRevisionStore, FsBlobStore, TursoIndex>;

/// Filesystem access grants local ownership; HTTP authentication stays in the server.
pub fn owner() -> Principal {
    Principal {
        actor: "local:owner".into(),
        scopes: vec![Scope::parse("admin:/").expect("constant scope")],
    }
}
pub fn open(base: &Path, vault: &str) -> DomainResult<LocalVault> {
    assemble(FsStore::open(base, vault)?)
}
pub fn create(base: &Path, vault: &str) -> DomainResult<LocalVault> {
    assemble(FsStore::create(base, vault)?)
}
pub fn open_or_create(base: &Path, vault: &str) -> DomainResult<LocalVault> {
    brains_adapter_fs::validate_vault_name(vault)?;
    if base
        .join("vaults")
        .join(vault)
        .join("format.json")
        .try_exists()
        .map_err(storage)?
    {
        open(base, vault)
    } else {
        create(base, vault)
    }
}
fn storage(e: std::io::Error) -> DomainError {
    DomainError::StorageUnavailable(e.to_string())
}
fn assemble(store: FsStore) -> DomainResult<LocalVault> {
    let index_path = store.vault_dir().join(INDEX_FILE);
    let other = store.vault_dir().join(OTHER_INDEX_FILE);
    if !index_path.try_exists().map_err(storage)? || other.try_exists().map_err(storage)? {
        // Publish a new derived index only after every existing HEAD was indexed.
        // An interrupted build is disposable and is never mistaken for a complete index.
        let building = store.vault_dir().join("index.turso-building.db");
        remove_index(&building)?;
        {
            let temporary = vault(store.clone(), TursoIndex::open(&building)?);
            // Turso drives local I/O in its futures. No network or Tokio reactor
            // is needed at this synchronous entry point.
            futures::executor::block_on(async {
                let mut cursor = None;
                loop {
                    let page = temporary
                        .reindex_page(
                            &owner(),
                            cursor.as_deref(),
                            temporary.limits.reindex_page_max,
                        )
                        .await?;
                    if !page.failed.is_empty() {
                        return Err(DomainError::IndexUnavailable(format!(
                            "index rebuild failed for {} paths",
                            page.failed.len()
                        )));
                    }
                    cursor = page.next_cursor;
                    if cursor.is_none() {
                        return Ok(());
                    }
                }
            })?;
            temporary.index.close()?;
        }
        // The checkpoint and connection close succeeded before publication.
        remove_index(&index_path)?;
        std::fs::rename(&building, &index_path).map_err(storage)?;
        remove_index(&building)?;
        // Only one feature variant remains active. Returning to another build
        // rebuilds from current HEADs, never reuses an abandoned stale index.
        remove_index(&other)?;
    }
    let index = TursoIndex::open(&index_path)?;
    Ok(vault(store, index))
}
fn vault(store: FsStore, index: TursoIndex) -> LocalVault {
    let name = store.vault().to_owned();
    Vault::new(
        FsHeadStore::new(store.clone()),
        FsRevisionStore::new(store.clone()),
        FsBlobStore::new(store),
        index,
        name,
        Limits::default(),
        CursorSigner::generate(),
    )
}

/// Derived database files only; caller holds the whole data-directory lock.
fn remove_index(path: &Path) -> DomainResult<()> {
    for suffix in ["", "-wal", "-shm"] {
        let mut file = path.as_os_str().to_os_string();
        file.push(suffix);
        match std::fs::remove_file(std::path::PathBuf::from(file)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(storage(error)),
        }
    }
    Ok(())
}
