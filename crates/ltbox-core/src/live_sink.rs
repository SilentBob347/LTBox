//! Process-wide live-log sink. Backstop for the Windows stdout pipe tap:
//! every `live!` call pushes into a shared `Mutex<Vec<String>>` that the
//! GUI subscription drains directly each tick — no pipe, no handle dance.

use std::sync::{Mutex, OnceLock};

const MAX_BUFFERED: usize = 4_096;

static SINK: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn buffer() -> &'static Mutex<Vec<String>> {
    SINK.get_or_init(|| Mutex::new(Vec::new()))
}

/// Append one fully-formatted log line. Used by the `live!` macro.
/// Bounded at [`MAX_BUFFERED`] entries — once a long-running flow
/// outpaces the GUI drain (drops/freezes), we discard the oldest
/// lines instead of unbounded growth that would OOM a 24h CI run.
pub fn push(line: String) {
    if let Ok(mut g) = buffer().lock() {
        push_into(&mut g, line);
    }
}

/// Bounded insert into an arbitrary line buffer. Extracted so unit
/// tests can exercise the drop-oldest policy on a private `Vec`
/// without racing the process-wide [`SINK`] that other modules touch
/// via `live!` while `cargo test` runs in parallel.
fn push_into(buf: &mut Vec<String>, line: String) {
    if buf.len() >= MAX_BUFFERED {
        let drop = buf.len() - MAX_BUFFERED + 1;
        buf.drain(..drop);
    }
    buf.push(line);
}

/// Take every queued line since the last drain, returning ownership
/// to the caller (typically the GUI subscription). Empty Vec when
/// nothing is pending; never blocks beyond the mutex acquisition.
pub fn drain() -> Vec<String> {
    if let Ok(mut g) = buffer().lock() {
        return std::mem::take(&mut *g);
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_into_appends_in_order() {
        // Local buffer only: a module-local mutex cannot isolate the
        // process-wide SINK from other modules' tests calling `live!`.
        let mut buf = Vec::new();
        push_into(&mut buf, "alpha".into());
        push_into(&mut buf, "beta".into());
        assert_eq!(buf, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn push_above_cap_drops_oldest() {
        let mut buf = Vec::new();
        for i in 0..(MAX_BUFFERED + 5) {
            push_into(&mut buf, format!("line {i}"));
        }
        assert_eq!(buf.len(), MAX_BUFFERED);
        // Oldest 5 dropped; first surviving line is `line 5`.
        assert_eq!(buf[0], "line 5");
        assert_eq!(buf.last().unwrap(), &format!("line {}", MAX_BUFFERED + 4));
    }
}
