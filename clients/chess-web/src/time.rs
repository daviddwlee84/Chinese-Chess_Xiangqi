//! Wall-clock timing helpers for the WASM frontend.
//!
//! `chess-ai` deliberately does **not** measure search wall-clock time
//! itself — `std::time::Instant::now()` panics on
//! `wasm32-unknown-unknown` (no clock in the WASM ABI), so the
//! library would either need a platform-conditional dep or to expose
//! the timing surface to callers. We took the latter route: each
//! caller (chess-web, chess-tui, ...) measures with its own native
//! clock and patches the result into `AiAnalysis::elapsed_ms` before
//! handing the analysis to the debug UI.
//!
//! On the web, the right clock is `window.performance.now()` —
//! sub-millisecond resolution, monotonic, unaffected by wall-clock
//! adjustments. We round to whole milliseconds and saturate to fit
//! the schema's `u32` (49 days of headroom).

/// Current `performance.now()` reading rounded to whole milliseconds.
///
/// Returns `0` if the `Window` or `Performance` interfaces are
/// unavailable (server-side rendering, headless tests). Callers
/// typically subtract two readings — `saturating_sub` handles the
/// degenerate "both 0" case cleanly so timing-disabled environments
/// just report `elapsed = 0` rather than panicking.
///
/// **Native build** (`cargo check --workspace` on a developer
/// machine, where the workspace target is x86_64 / aarch64) compiles
/// the chess-web crate's pure-logic modules without the wasm-gated
/// `web-sys` dependency — under this configuration `perf_now_ms`
/// stubs to `0` and any `elapsed = end - start` reads as `0 ms`.
/// The function exists on native so debug_panel-adjacent test
/// fixtures can call it without further cfg gating.
#[cfg(target_arch = "wasm32")]
pub fn perf_now_ms() -> u32 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now() as u32).unwrap_or(0)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn perf_now_ms() -> u32 {
    0
}

/// Format an elapsed-milliseconds reading for display in the AI
/// debug panel. Compact and at-a-glance:
///
/// | input  | output     |
/// |--------|------------|
/// | `0`    | `"0 ms"`   |
/// | `423`  | `"423 ms"` |
/// | `999`  | `"999 ms"` |
/// | `1000` | `"1.0 s"`  |
/// | `2345` | `"2.3 s"`  |
/// | `60_000` | `"60.0 s"` |
///
/// Rationale: sub-second values benefit from precise integer ms
/// (perceptible difference at 100 vs 200 ms); above one second the
/// user is firmly in "noticeable wait" territory and one decimal of
/// seconds is enough resolution.
pub fn format_elapsed_ms(ms: u32) -> String {
    if ms < 1000 {
        format!("{} ms", ms)
    } else {
        format!("{:.1} s", ms as f64 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_under_one_second_uses_ms() {
        assert_eq!(format_elapsed_ms(0), "0 ms");
        assert_eq!(format_elapsed_ms(1), "1 ms");
        assert_eq!(format_elapsed_ms(423), "423 ms");
        assert_eq!(format_elapsed_ms(999), "999 ms");
    }

    #[test]
    fn format_one_second_and_above_uses_one_decimal_s() {
        assert_eq!(format_elapsed_ms(1000), "1.0 s");
        assert_eq!(format_elapsed_ms(1500), "1.5 s");
        assert_eq!(format_elapsed_ms(2345), "2.3 s");
        assert_eq!(format_elapsed_ms(60_000), "60.0 s");
    }

    #[test]
    fn format_handles_large_values_without_panic() {
        // Worst-case: u32::MAX ms (~49 days). Sanity that we don't
        // panic on overflow — the f64 cast is wide enough.
        let _ = format_elapsed_ms(u32::MAX);
    }
}
