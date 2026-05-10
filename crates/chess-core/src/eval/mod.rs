//! Evaluation primitives shared by the threat-highlight UI and (eventually)
//! the search-based AI.
//!
//! For now this module exposes:
//!
//! * [`piece_value`] — a small fixed-point value table for a piece on a
//!   given square. Used by the SEE and threat-detection helpers below;
//!   not (yet) by the AI proper, which has its own evaluator stack
//!   (`crates/chess-ai`). Numbers are deliberately conservative so they
//!   read well as "centi-pawn-ish" rather than as a tuned eval.
//! * [`see`] — Static Exchange Evaluation, simulating an exchange of
//!   captures on a single target square, returning the predicted net
//!   gain for the initiating attacker side. Variant-aware: xiangqi
//!   uses ray attacks + cannon screens; banqi uses revealed pieces +
//!   rank rules.
//!
//! The threat-detection helpers (`attacked_pieces`, `net_loss_pieces`,
//! `mate_threat_pieces`) live next to the per-variant move generators
//! in [`crate::rules`] — they share too much code with `is_attacked`
//! to live anywhere else. They consume `piece_value` / `see` from
//! here.
//!
//! ### SEE value choices
//!
//! These values are intentionally simple and not tuned against game
//! data. The downstream consumer (Mode B "被捉" highlight) only cares
//! about the **sign** of `see()`, not its magnitude — a defended
//! chariot trade returning 0 must register as "safe" while an
//! undefended chariot returning +9 must register as "in danger". Any
//! ordering that keeps:
//!
//! ```text
//! General >> Chariot > Cannon ≈ Horse > Advisor ≈ Elephant > Soldier
//! ```
//!
//! and gives the past-river soldier a small bump produces the same
//! UX. We pick whole numbers so the negamax retract step in `see()`
//! stays exact (no floating-point drift). See
//! `backlog/threat-highlight-feature.md` for the (deferred) discussion
//! of making this user-configurable.

pub mod see;

pub use see::{piece_value, see, SEE_GENERAL_VALUE};
