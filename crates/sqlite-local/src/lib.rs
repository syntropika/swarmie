//! One lifecycle-owned SQLite connection for independent local persistence domains.
mod database;

mod service;

pub use database::{Configuration, Database, Error, Schema};

pub use service::{
    DatabaseHandle, SqliteLocal, SqliteLocalIdentity, SqliteLocalInput, SqliteLocalProvider,
    SqliteLocalRef, facet,
};
