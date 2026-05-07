//! `ltbox-core` — domain layer shared across LTBox crates.
//!
//! Config loader, AES-CBC `.x` decryption, GitHub client, i18n, and
//! rawprogram XML parser. Every fallible API returns [`Result<T>`] /
//! [`LtboxError`]. Port of the non-UI parts of Python LTBox v2.x.

pub mod app_paths;
pub mod config;
pub mod crypto;
pub mod downloader;
pub mod error;
pub mod github;
pub mod i18n;
pub mod lenovo_info;
pub mod lenovo_ota;
pub mod live_sink;
pub mod partition_lun;
pub mod runtime;
pub mod sahara_xml;
pub mod xml_catalog;

pub use error::{LtboxError, Result};

/// Echo a line to `println!` (terminal + GUI stdout-tap), the
/// [`live_sink`] in-process queue (drained by the GUI subscription),
/// AND the caller's `&mut Vec<String>` for the rare consumer that
/// inspects the buffer post-flow (driver installer log, headless
/// tests, …).
///
/// `*ExecDone` handlers that previously fed this Vec straight back
/// into `App::log_extend` should NOT do so — the sink path already
/// streamed every line and a second walk re-appends the entire
/// transcript on top of itself. Drain the sink + tap one final time
/// instead and discard the Vec.
///
/// Lives in `ltbox-core` so every downstream crate (`ltbox-device`,
/// `ltbox-patch`, `ltbox-gui`) can emit through the same path without
/// redefining the macro or taking a circular dep. `#[macro_export]`
/// puts it at the crate root, reachable as `ltbox_core::live!(…)`.
#[macro_export]
macro_rules! live {
    ($log:expr, $($arg:tt)*) => {{
        let _line = format!($($arg)*);
        println!("{}", _line);
        $crate::live_sink::push(_line.clone());
        $log.push(_line);
    }};
}

/// Translate `$key` and substitute `{name}` placeholders from a list of
/// `name = value` pairs. Eliminates the chain of
/// `tr("k").replace("{a}", &x).replace("{b}", &y)…` repeated across
/// every live-log emit site.
///
/// Each `value` is converted via `Display` (`format!("{}", v)`) so the
/// caller can pass `&str`, `String`, integers, floats, or anything else
/// implementing `Display`. For pre-formatted floats (`"{x:.1}"`) just
/// pass `&format!("{x:.1}")`.
///
/// ```ignore
/// live!(
///     log,
///     "[Driver] {}",
///     tr_args!(
///         "live_driver_progress_pct",
///         name = display_name,
///         pct = format!("{pct:>3}"),
///         downloaded = format!("{dl_mb:.1}"),
///     )
/// );
/// ```
#[macro_export]
macro_rules! tr_args {
    ($key:expr $(, $name:ident = $val:expr)* $(,)?) => {{
        let mut __s = $crate::i18n::tr($key);
        $(
            __s = __s.replace(
                concat!("{", stringify!($name), "}"),
                &format!("{}", $val),
            );
        )*
        __s
    }};
}
