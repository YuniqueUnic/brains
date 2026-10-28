//! Application-layer business tests over the real local adapter stack.
//! Fault injection uses cfg(test) wrappers; production code stays clean.

mod commit;
mod content;
mod patch;
mod reindex;
mod resources;
mod search;
mod seed;
mod support;
mod transfer;
