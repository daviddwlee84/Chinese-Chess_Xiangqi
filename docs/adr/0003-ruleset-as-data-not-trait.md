# ADR 0003: `RuleSet` is plain data, not a trait

## Context

Five banqi house rules combine independently with one another, with three xiangqi-style variants, and with three-kingdoms. The natural OO instinct is "trait `RuleSet` and stack decorators" — `StandardRules ∘ ChainCapture ∘ ChariotRush`.

In practice with Rust this hurts:

- Trait objects (`Box<dyn RuleSet>`) kill inlining on the move-gen hot path that AI will spam.
- Generic `RuleSet: Sized` means `GameState` becomes generic, which propagates through the entire net protocol — there's no single `SaveGame` type to ship.
- Serde for trait objects requires `typetag` and a global registry.
- The set of rules is **closed**: we own all of them. No third party adds rules dynamically.

## Decision

```rust
pub enum Variant { Xiangqi, Banqi, ThreeKingdomBanqi }

bitflags::bitflags! {
    pub struct HouseRules: u32 {
        const CHAIN_CAPTURE     = 1 << 0;
        const DARK_CHAIN        = 1 << 1;
        const CHARIOT_RUSH      = 1 << 2;
        const HORSE_DIAGONAL    = 1 << 3;
        const CANNON_FAST_MOVE  = 1 << 4;
    }
}

pub struct RuleSet {
    pub variant: Variant,
    pub house:   HouseRules,
    pub draw_policy: DrawPolicy,
    pub banqi_seed: Option<u64>,
}
```

Move generation is free functions:

```rust
pub fn generate_moves(state: &GameState, out: &mut MoveList) {
    match state.rules.variant {
        Variant::Xiangqi          => xiangqi::generate(state, out),
        Variant::Banqi            => banqi::generate(state, out),
        Variant::ThreeKingdomBanqi => three_kingdom::generate(state, out),
    }
}
```

Inside `banqi::generate`, each house rule is a `if state.rules.house.contains(...) { gen_xxx(state, out) }` branch. Adding a new rule = new bitflag + new generator function.

## Consequences

- One concrete `GameState` type. Serde just works.
- Optimizer inlines the lot.
- Rule combinatorics is data, not a type.
- Tests can fuzz over `HouseRules` bit patterns directly with proptest.

## Rejected alternatives

- `trait RuleSet`: see Context.
- `Box<dyn Fn(&GameState, &mut MoveList)>`: same downsides as trait objects, with worse ergonomics.
