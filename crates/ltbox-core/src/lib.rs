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

/// Echo a line to `println!` so the GUI's stdout tap can stream it to
/// the live log panel immediately instead of buffering it in the
/// returned `Vec<String>` log until the whole op ends.
///
/// `$log` is accepted for call-site ergonomics (callers already thread a
/// `&mut Vec<String>` through every step) but intentionally ignored —
/// pushing here would double-render in the GUI, which re-drains the
/// returned Vec on top of what the tap already captured. When the tap
/// is off (CLI / tests), `println!` alone is still the right sink.
///
/// Lives in `ltbox-core` so every downstream crate (`ltbox-device`,
/// `ltbox-patch`, `ltbox-gui`) can emit through the same path without
/// redefining the macro or taking a circular dep. `#[macro_export]`
/// puts it at the crate root, reachable as `ltbox_core::live!(…)`.
#[macro_export]
macro_rules! live {
    ($log:expr, $($arg:tt)*) => {{
        let _ = &$log;
        println!($($arg)*);
    }};
}
