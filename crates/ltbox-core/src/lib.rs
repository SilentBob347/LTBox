//! `ltbox-core` — domain layer shared across LTBox crates.
//!
//! Config loader, AES-CBC `.x` decryption, GitHub client, i18n, and
//! rawprogram XML parser. Every fallible API returns [`Result<T>`] /
//! [`LtboxError`]. Port of the non-UI parts of Python LTBox v2.x.

pub mod config;
pub mod crypto;
pub mod downloader;
pub mod error;
pub mod github;
pub mod i18n;
pub mod runtime;
pub mod xml_catalog;

pub use error::{LtboxError, Result};
