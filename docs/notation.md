# Notation

> **Status**: skeleton.

## ICCS (Internet Chinese Chess Server)

Coordinate-based, ASCII-friendly. Files `a..i` left-to-right from Red's view, ranks `0..9` bottom-to-top.

```
h2e2     # 炮二平五: cannon on h2 moves to e2
b0c2     # 馬八進七: horse on b0 jumps to c2
```

This is the primary input notation for the CLI test harness in PR 1.

## WXF (World Xiangqi Federation)

Chinese-language traditional notation. Format: `<piece><file><action><target>`.

```
炮二平五    # cannon on (red) file 2 moves horizontally to file 5
馬八進七    # horse on (red) file 8 advances to file 7
```

Files are numbered 1–9 right-to-left from each player's own perspective. Implementation deferred to PR 2.

## Banqi Notation (ad-hoc)

```
flip a3       # reveal the face-down piece at a3 (Move::Reveal)
a3 b3         # move face-up piece a3 to b3 (non-capture, Move::Step)
a3xb3         # capture (Move::Capture)
a3x?b3        # 暗吃 — atomic dark-capture: a3 attacks face-down b3, engine
              # resolves Capture / Probe / Trade at apply-time (Move::DarkCapture)
a3xb3xc3xd3   # atomic chain capture (Move::ChainCapture). NOT user-facing
              # in the current clients — the live UX is step-by-step under
              # the engine's chain_lock state machine (each hop is its own
              # `axb`-form move).
end a3        # explicit end-chain terminator for 連吃 mode — sent by the
              # client when chain_lock is active and the player wants to
              # release without further captures (Move::EndChain).
```

`Move::DarkCapture` and `Move::EndChain` were added in protocol v5
alongside the engine-driven chain mode (ADR-0008). `EndChain` is also
issued implicitly by the chess-tui / chess-web clients when the player
clicks the chain-locked piece itself.
