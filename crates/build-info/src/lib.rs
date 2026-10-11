//! Shared build identity for the versioned Brains workspace.

/// Product version and Git metadata captured when this workspace was built.
pub const VERSION: &str = env!("BRAINS_BUILD_VERSION");
