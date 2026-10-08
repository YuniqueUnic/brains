mod blob;
mod head;
mod revision;

use crate::FsStore;

pub(crate) fn temp_store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::create(dir.path(), "default").unwrap();
    (dir, store)
}
