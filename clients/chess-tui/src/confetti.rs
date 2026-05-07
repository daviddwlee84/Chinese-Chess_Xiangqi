//! Confetti animation: a tiny particle system written specifically for the
//! ratatui draw loop. No `rand` crate dep — a small linear-congruential RNG
//! seeded from `Instant::now()` is plenty for ~40 visual particles spawned
//! once per game-end.
//!
//! Lifecycle:
//!  1. `ConfettiAnim::spawn(area)` — caller sees a status transition and
//!     creates the animation. Particles start near the top edge of the
//!     board area, biased upward (negative `vy`) so the burst rises before
//!     gravity pulls it down.
//!  2. `step()` advances each particle by `(vx, vy)` and applies gravity.
//!     Particles that leave the area on any side are dropped. Returns the
//!     `done` flag — true once the lifetime cap (3s) elapses or the vec is
//!     empty.
//!  3. `render(buf, area)` writes glyphs into the ratatui `Buffer` cells
//!     they cover. Multiple particles in the same cell silently overwrite
//!     (last write wins) — no flicker because we step + draw in one frame.
//!
//! The renderer also draws a centered ASCII-art banner from `banner.rs`
//! during the same window; together they make the game-over moment feel
//! emphatic without us shipping a sprite engine.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

const LIFETIME: Duration = Duration::from_millis(3000);
const SPAWN_COUNT: usize = 48;
const GRAVITY: f32 = 0.35;
const GLYPHS: &[char] = &['*', '+', '✦', '✧', '◆', '●', '✺'];
const COLORS: &[Color] = &[
    Color::Yellow,
    Color::LightRed,
    Color::LightCyan,
    Color::LightGreen,
    Color::LightMagenta,
    Color::White,
];

#[derive(Clone, Debug)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub glyph: char,
    pub color: Color,
}

#[derive(Debug)]
pub struct ConfettiAnim {
    pub particles: Vec<Particle>,
    started_at: Instant,
}

impl ConfettiAnim {
    /// Spawn a fresh burst sized to fit `area`. The caller decides where the
    /// area is — typically the board sub-rect.
    pub fn spawn(area: Rect) -> Self {
        let now = Instant::now();
        let mut rng = LcgRng::seed_from_instant(now);
        let mut particles = Vec::with_capacity(SPAWN_COUNT);
        let cx = area.x as f32 + area.width as f32 / 2.0;
        let cy = area.y as f32 + area.height as f32 / 2.0;
        for _ in 0..SPAWN_COUNT {
            // Spread origin across the upper half of the area so the burst
            // doesn't all stem from a single cell.
            let x = cx + rng.range(-(area.width as f32) / 3.0, area.width as f32 / 3.0);
            let y = cy + rng.range(-(area.height as f32) / 4.0, 0.0);
            let vx = rng.range(-0.7, 0.7);
            let vy = rng.range(-1.6, -0.5);
            let glyph = GLYPHS[rng.usize_below(GLYPHS.len())];
            let color = COLORS[rng.usize_below(COLORS.len())];
            particles.push(Particle { x, y, vx, vy, glyph, color });
        }
        Self { particles, started_at: now }
    }

    /// Advance one frame. Returns `true` once the lifetime is exhausted or
    /// every particle has left the area — the caller should drop the anim
    /// when this is true.
    pub fn step(&mut self, area: Rect) -> bool {
        for p in self.particles.iter_mut() {
            p.x += p.vx;
            p.y += p.vy;
            p.vy += GRAVITY;
        }
        // Drop offscreen particles.
        let x_min = area.x as f32 - 1.0;
        let x_max = (area.x + area.width) as f32 + 1.0;
        let y_max = (area.y + area.height) as f32 + 1.0;
        self.particles.retain(|p| p.x >= x_min && p.x <= x_max && p.y <= y_max);
        self.is_done()
    }

    pub fn is_done(&self) -> bool {
        self.particles.is_empty() || self.started_at.elapsed() >= LIFETIME
    }

    /// Write particles into the buffer cells they cover. Cells outside
    /// `area` are skipped defensively — the spawn / step logic should keep
    /// particles inside, but a resize between frames could shrink the area.
    pub fn render(&self, buf: &mut Buffer, area: Rect, use_color: bool) {
        for p in &self.particles {
            let x = p.x.round() as i32;
            let y = p.y.round() as i32;
            if x < area.x as i32
                || x >= (area.x + area.width) as i32
                || y < area.y as i32
                || y >= (area.y + area.height) as i32
            {
                continue;
            }
            let cell = &mut buf[(x as u16, y as u16)];
            cell.set_char(p.glyph);
            if use_color {
                cell.set_style(Style::default().fg(p.color).add_modifier(Modifier::BOLD));
            } else {
                cell.set_style(Style::default().add_modifier(Modifier::BOLD));
            }
        }
    }
}

// ---- Tiny LCG -----------------------------------------------------------
//
// xorshift32 is just enough for visual jitter; it is not cryptographic and
// not seeded for reproducibility. We seed from `Instant` so each spawn
// looks different.

struct LcgRng(u32);

impl LcgRng {
    fn seed_from_instant(t: Instant) -> Self {
        // Use the elapsed-since-program-start nanos mod u32 as a seed. Two
        // bursts in the same millisecond would still differ because nanos
        // drift; if seed lands on 0 (impossibly rare) we substitute 1.
        let nanos = t.elapsed().as_nanos() as u32;
        let s = if nanos == 0 { 0x9E3779B9 } else { nanos };
        LcgRng(s)
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        if x == 0 {
            x = 0x9E3779B9;
        }
        self.0 = x;
        x
    }

    fn unit_f32(&mut self) -> f32 {
        (self.next_u32() as f32) / (u32::MAX as f32)
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.unit_f32() * (hi - lo)
    }

    fn usize_below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u32() as usize) % n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::new(0, 0, 60, 30)
    }

    #[test]
    fn spawn_creates_particles() {
        let anim = ConfettiAnim::spawn(area());
        assert!(!anim.particles.is_empty());
        assert!(anim.particles.len() <= SPAWN_COUNT);
        // All particles start inside or just above the area horizontally.
        for p in &anim.particles {
            assert!(p.x >= -1.0 && p.x <= 70.0, "x out of plausible bounds: {}", p.x);
        }
    }

    #[test]
    fn step_eventually_clears_or_exits_lifetime() {
        let mut anim = ConfettiAnim::spawn(area());
        // 60 frames at default gravity should push everything off the bottom.
        for _ in 0..200 {
            if anim.step(area()) {
                break;
            }
        }
        assert!(anim.is_done(), "anim should be done after enough frames");
    }

    #[test]
    fn render_does_not_panic_on_resize() {
        let big = area();
        let small = Rect::new(0, 0, 5, 5);
        let anim = ConfettiAnim::spawn(big);
        let mut buf = Buffer::empty(small);
        // Particles spawned for `big` should be filtered when rendering into `small`.
        anim.render(&mut buf, small, true);
    }
}
