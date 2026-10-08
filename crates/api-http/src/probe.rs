//! TEMP probe: locate the non-Send component. Delete after diagnosis.
use brains_application::Vault;
use brains_domain::{Principal, VaultPath};
use brains_ports::{BlobStore, HeadStore, IndexStore, RevisionStore};

#[allow(unused)]
fn probe<
    H: HeadStore + Send + Sync,
    R: RevisionStore + Send + Sync,
    B: BlobStore + Send + Sync,
    I: IndexStore + Send + Sync,
>(
    vault: &Vault<H, R, B, I>,
    p: &Principal,
) {
    fn is_send<T: Send>(_: &T) {}
    let path = VaultPath::trusted("a.md".into());
    let fut = vault.read_entry(p, &path);
    is_send(&fut);
}
