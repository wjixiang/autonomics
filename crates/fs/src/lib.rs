pub mod storage;
pub mod tools;

pub use storage::OpendalFileStorage;
pub use tools::file_base_registrations;

// Re-export so downstream crates can reference opendal error/operator types
// without taking a direct dependency on the opendal crate.
pub use opendal;
