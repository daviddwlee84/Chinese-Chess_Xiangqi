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
flip a3       # reveal the face-down piece at a3
a3 b3         # move face-up piece a3 to b3 (non-capture)
a3xb3         # capture
a3xb3xc3xd3   # chain capture (whole chain is one move)
```
