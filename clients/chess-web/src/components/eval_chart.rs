//! Post-game win-rate trend chart — SVG line chart of every recorded
//! [`EvalSample`] across the game.
//!
//! Mounted by `pages/local.rs` inside the sidebar (below the move
//! history panel) when the game has ended (`status != Ongoing`) AND
//! `?evalbar=1` is on AND at least one sample exists. Hidden during
//! play to avoid distracting the player; the live eval bar covers
//! that role.
//!
//! Pure inline SVG — no chart library. Y axis is **Red wins %**
//! (0–100, clamped per `chess_ai::cp_to_win_pct`), X axis is ply
//! number (auto-scales from 0 to `samples.len()-1`). 50 % gridline
//! drawn as a faint horizontal reference. Hover tooltip via SVG
//! `<title>` element on each sample point.
//!
//! No interactivity beyond hover-for-tooltip — a richer scrubbing UI
//! is in P3 backlog (`chess-web: history scrubber + undo via UI`).

use leptos::*;

use crate::eval::EvalSample;

const CHART_WIDTH: f32 = 260.0;
const CHART_HEIGHT: f32 = 140.0;
const PADDING_X: f32 = 18.0; // room for Y-axis labels (left only — no right axis)
const PADDING_TOP: f32 = 8.0;
const PADDING_BOTTOM: f32 = 18.0; // room for X-axis ply labels

#[component]
pub fn EvalChart(
    /// Full sample vector, in chronological order. The chart renders
    /// the polyline `(ply_x, red_win_pct_y)` connecting every entry.
    /// Empty input → component renders nothing (caller should gate).
    #[prop(into)]
    samples: Signal<Vec<EvalSample>>,
) -> impl IntoView {
    let path_d = move || samples.with(build_polyline_path);
    let area_d = move || samples.with(build_red_area_path);
    let circles = move || {
        samples.with(|s| {
            // Draw a hover-target circle on every sample so tooltips
            // are reachable. Keep them semi-transparent so the line
            // is still the primary visual.
            s.iter()
                .enumerate()
                .map(|(i, sample)| {
                    let (x, y) = sample_to_xy(s.len(), i, sample);
                    let r = if i == s.len() - 1 { 3.0 } else { 1.8 };
                    let title = format!(
                        "Ply {}: {}% Red ({:+} cp from {} POV)",
                        sample.ply,
                        (sample.red_win_pct * 100.0).round() as i32,
                        sample.cp_stm_pov,
                        match sample.side_to_move_at_pos {
                            chess_core::piece::Side::RED => "Red 紅",
                            chess_core::piece::Side::BLACK => "Black 黑",
                            _ => "Green 綠",
                        },
                    );
                    view! {
                        <circle
                            cx=x.to_string()
                            cy=y.to_string()
                            r=r.to_string()
                            class="eval-chart__dot"
                        >
                            <title>{title}</title>
                        </circle>
                    }
                })
                .collect_view()
        })
    };

    // Show the latest cp/win% as a footer caption so the chart's
    // current value is obvious without hovering.
    let summary = move || {
        samples.with(|s| match s.last() {
            None => "No samples".to_string(),
            Some(last) => format!(
                "Last: ply {} → Red {}% ({:+} cp)",
                last.ply,
                (last.red_win_pct * 100.0).round() as i32,
                last.cp_stm_pov,
            ),
        })
    };

    let viewbox = format!("0 0 {} {}", CHART_WIDTH, CHART_HEIGHT);

    view! {
        <section class="eval-chart" aria-label="Game win-rate trend chart">
            <h4 class="eval-chart__title">"Win-rate trend 勝率走勢"</h4>
            <svg
                class="eval-chart__svg"
                viewBox=viewbox
                preserveAspectRatio="none"
                role="img"
            >
                // Background panels — top half black-tinted, bottom half red-tinted.
                <rect
                    x=PADDING_X.to_string()
                    y=PADDING_TOP.to_string()
                    width=(CHART_WIDTH - PADDING_X).to_string()
                    height=((CHART_HEIGHT - PADDING_TOP - PADDING_BOTTOM) / 2.0).to_string()
                    class="eval-chart__bg-black"
                />
                <rect
                    x=PADDING_X.to_string()
                    y=(PADDING_TOP + (CHART_HEIGHT - PADDING_TOP - PADDING_BOTTOM) / 2.0).to_string()
                    width=(CHART_WIDTH - PADDING_X).to_string()
                    height=((CHART_HEIGHT - PADDING_TOP - PADDING_BOTTOM) / 2.0).to_string()
                    class="eval-chart__bg-red"
                />
                // 50% reference line.
                <line
                    x1=PADDING_X.to_string()
                    x2=CHART_WIDTH.to_string()
                    y1=(PADDING_TOP + (CHART_HEIGHT - PADDING_TOP - PADDING_BOTTOM) / 2.0).to_string()
                    y2=(PADDING_TOP + (CHART_HEIGHT - PADDING_TOP - PADDING_BOTTOM) / 2.0).to_string()
                    class="eval-chart__gridline"
                />
                // Y-axis labels (just 100 / 50 / 0 — Red %).
                <text
                    x="2"
                    y=(PADDING_TOP + 4.0).to_string()
                    class="eval-chart__axis-label"
                >"100"</text>
                <text
                    x="2"
                    y=(PADDING_TOP + (CHART_HEIGHT - PADDING_TOP - PADDING_BOTTOM) / 2.0 + 3.0).to_string()
                    class="eval-chart__axis-label"
                >"50"</text>
                <text
                    x="6"
                    y=(CHART_HEIGHT - PADDING_BOTTOM + 3.0).to_string()
                    class="eval-chart__axis-label"
                >"0"</text>
                // X-axis label (just the last ply count, centered)
                <text
                    x=(CHART_WIDTH / 2.0).to_string()
                    y=(CHART_HEIGHT - 4.0).to_string()
                    class="eval-chart__axis-label eval-chart__axis-label--ply"
                    text-anchor="middle"
                >"Ply →"</text>
                // Filled area under the curve, capped at the 50% line —
                // a subtle "Red advantage zone" visualisation.
                <path d=area_d class="eval-chart__area"/>
                // The actual line.
                <path d=path_d class="eval-chart__line"/>
                // Sample dots with hover tooltips.
                {circles}
            </svg>
            <p class="eval-chart__summary muted">{summary}</p>
        </section>
    }
}

/// Map a sample index to its (x, y) position in the chart's viewBox.
/// Y axis: 0 % red → bottom of plot area, 100 % red → top. X axis:
/// linear from 0 to N-1 (or single-sample = centred).
fn sample_to_xy(total: usize, idx: usize, sample: &EvalSample) -> (f32, f32) {
    let plot_w = CHART_WIDTH - PADDING_X;
    let plot_h = CHART_HEIGHT - PADDING_TOP - PADDING_BOTTOM;
    let x_frac = if total <= 1 { 0.5 } else { idx as f32 / (total - 1) as f32 };
    let x = PADDING_X + x_frac * plot_w;
    // y_frac: 1.0 = top (100% red), 0.0 = bottom (0% red).
    let y_frac = sample.red_win_pct.clamp(0.0, 1.0);
    let y = PADDING_TOP + (1.0 - y_frac) * plot_h;
    (x, y)
}

/// Build the SVG path "M x0,y0 L x1,y1 L x2,y2 …" connecting every
/// sample with straight lines.
fn build_polyline_path(samples: &Vec<EvalSample>) -> String {
    if samples.is_empty() {
        return String::new();
    }
    let total = samples.len();
    let mut s = String::with_capacity(samples.len() * 18);
    for (i, sample) in samples.iter().enumerate() {
        let (x, y) = sample_to_xy(total, i, sample);
        if i == 0 {
            s.push_str(&format!("M{:.2},{:.2}", x, y));
        } else {
            s.push_str(&format!(" L{:.2},{:.2}", x, y));
        }
    }
    s
}

/// Build a closed area path that fills the region BETWEEN the curve
/// and the 50 % midline. Renders as a translucent "advantage shading"
/// — when the line is above 50 %, the area between line and midline
/// gets red-tinted; when below, black-tinted. (We use a single fill
/// colour and let the line itself convey direction; the shading just
/// adds visual weight to the swing.)
fn build_red_area_path(samples: &Vec<EvalSample>) -> String {
    if samples.is_empty() {
        return String::new();
    }
    let total = samples.len();
    let plot_h = CHART_HEIGHT - PADDING_TOP - PADDING_BOTTOM;
    let midline_y = PADDING_TOP + plot_h / 2.0;
    let mut s = String::with_capacity(samples.len() * 18);
    // First point: start at midline below first sample.
    let (x0, _) = sample_to_xy(total, 0, &samples[0]);
    s.push_str(&format!("M{:.2},{:.2}", x0, midline_y));
    for (i, sample) in samples.iter().enumerate() {
        let (x, y) = sample_to_xy(total, i, sample);
        s.push_str(&format!(" L{:.2},{:.2}", x, y));
    }
    // Close back along the midline to the start.
    let (xn, _) = sample_to_xy(total, total - 1, &samples[total - 1]);
    s.push_str(&format!(" L{:.2},{:.2} Z", xn, midline_y));
    s
}
