//! Pure-CSS confetti burst. Each particle is a `<div>` with three inline
//! CSS variables (`--x` start position, `--delay` stagger, `--color`
//! fill) consumed by the `@keyframes confetti-fall` animation in
//! `style.css`. No canvas, no JS animation loop — once mounted the
//! browser does the rest.
//!
//! Lifetime is owned by the parent: mount the component when you want
//! the burst, unmount it ~3 seconds later. The `show` signal handles the
//! mount/unmount via `<Show>` so re-firing works cleanly (mount = new
//! seed = new random positions).

use leptos::*;

const PARTICLE_COUNT: usize = 48;
const COLORS: &[&str] = &["#fcd34d", "#f87171", "#34d399", "#60a5fa", "#c084fc", "#fb923c"];

#[component]
pub fn Confetti() -> impl IntoView {
    // Generate per-particle inline-style strings once on mount. We use
    // `js_sys::Math::random` which is the same generator the browser uses
    // for its own entropy — good enough for visual jitter, no crate dep.
    let particles = (0..PARTICLE_COUNT)
        .map(|i| {
            let x = rand_f64() * 100.0;
            let delay = rand_f64() * 0.8;
            let drift = (rand_f64() - 0.5) * 60.0;
            let rotate_end = (rand_f64() - 0.5) * 1440.0;
            let color = COLORS[i % COLORS.len()];
            // Two CSS vars carry layout (start x, drift), two carry
            // animation timing (delay, end rotation). The duration stays
            // fixed at 3s so the parent's unmount lines up with the
            // visual end of the burst.
            format!(
                "--x:{x:.2}%;--drift:{drift:.0}px;--delay:{delay:.2}s;--rotate-end:{rotate_end:.0}deg;--color:{color};"
            )
        })
        .collect::<Vec<_>>();

    view! {
        <div class="confetti-container" aria-hidden="true">
            {particles
                .into_iter()
                .map(|style| view! { <div class="confetti-particle" style=style></div> })
                .collect_view()}
        </div>
    }
}

fn rand_f64() -> f64 {
    js_sys::Math::random()
}
