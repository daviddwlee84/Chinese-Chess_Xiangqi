# Pitfalls

Past traps we've stepped on. **Symptoms-first** knowledge base — when a
problem recurs (on a new machine, after an upgrade, with a new tool combo),
grepping the symptom here lands you on the root cause and workaround in
seconds, instead of re-debugging from scratch.

This folder is excluded from <DEPLOYMENT/PACKAGING MECHANISM, e.g. chezmoi via
.chezmoiignore.tmpl, Python via MANIFEST.in, npm via .npmignore> — it is repo
metadata for maintainers, not user-facing config to deploy.

## Pitfalls vs the rest

| Surface | Time direction | Question it answers | Access pattern |
|---|---|---|---|
| `docs/<tool>.md` | Present | "How does this tool work / how do I configure it?" | Read top to bottom |
| `pitfalls/<slug>.md` | **Past** | **"I see error X — has this happened before?"** | **Grep symptoms** |
| `backlog/<slug>.md` | Future | "We thought about doing Y — what was the analysis?" | Index in `TODO.md` |
| `AGENTS.md` Hard invariants | Present | "What rules MUST agents follow?" | Read top to bottom |

A pitfall **graduates** to a Hard invariant when the trap is serious enough
that you can't rely on memory or grep — typically when (a) it recurs across
machines, (b) it silently corrupts state, or (c) the workaround is non-obvious
and easy to undo by accident. When graduating, leave a `pitfalls/<slug>.md`
as historical record and link to it from the new invariant.

## When to add a pitfall doc

Add `pitfalls/<slug>.md` when you've spent more than ~15 minutes on something
that wasn't googleable, AND any of:

- The symptom is non-obvious from the root cause (silent state, weird side
  effect, behaviour change without error)
- The fix is "do nothing different but in a specific order"
- The same trap could be hit by a new agent / new machine / new contributor
- An upstream bug exists with no ETA — workaround needs to outlive memory
- A specific tool version is required (or forbidden) and failure at the
  wrong version is silent / confusing

## When NOT to add a pitfall doc

- Trivially googleable (next person solves in 30 seconds)
- Already covered in `docs/<tool>.md` — cross-link from this README's
  "Cross-referenced pitfalls" table below instead of duplicating
- Already a Hard invariant (cross-link only)
- One-off transient (network glitch, machine-specific config rot)

## File template

See [`pitfall-doc.md.template`] in the
[`project-knowledge-harness` skill](https://github.com/daviddwlee84/agent-skills/tree/main/skills/local/project-knowledge-harness)
for the per-doc template.

Key sections (different from `backlog/` template — symptom-first, not
context-first):

```markdown
# <Title describing the SYMPTOM, not the root cause>

**Symptoms** (grep this section): <verbatim error messages, observable behaviour>
**First seen**: YYYY-MM
**Affects**: <tool/version/OS combo>
**Status**: workaround documented / fixed upstream in vX.Y / WONTFIX

## Symptom

Full error messages (verbatim — preserves grep-ability).

## Root cause

Why this happens, with source/docs/upstream issue link.

## Workaround

Copy-pasteable commands or config diff.

## Prevention

How to avoid stepping on this again.

## Related

Links to docs, sibling pitfalls, TODO entries, upstream issues.
```

## Index

Pitfalls owned by this folder. Keep alphabetical.

| Slug | Symptom keywords | Status |
|---|---|---|
| [`ai-search-depth-setting-shows-depth-4`](ai-search-depth-setting-shows-depth-4.md) | Search depth (advanced) 10 displays as Depth 4, picker depth setting ignored, AI Debug panel reached_depth 4, NODE_BUDGET 250000, v5 iterative deepening budget bail | fixed (visibility + scaled budget + Custom-radio picker UX) |
| [`eval-sample-stale-analysis-race`](eval-sample-stale-analysis-race.md) | win-rate display 1%/99% never reached, eval bar stops at ~88% even at mate-in-1, sidebar `紅 % • 黑 %` badge stuck on stale percentage, sample-write reactive effect, `?evalbar=1` wrong cp, `push_or_replace_sample` dedup-by-ply | fixed in `fc51f24` (2026-05-11) |
| [`ios-safari-svg-click-no-tap`](ios-safari-svg-click-no-tap.md) | iOS Safari taps don't select chess pieces, Chrome on iOS works but Safari doesn't, Copy / Find Selection / Look Up callout on SVG board, `:hover` ghost click, `on:click` on `<rect>` no fire, `on:pointerup` workaround | workaround documented |
| [`leptos-router-base-trailing-slash`](leptos-router-base-trailing-slash.md) | blank `<main class="app-shell">` at project base URL, GitHub Pages root white screen, Playwright `wait-for-selector` timeout, `<Router base=…>` no match | workaround documented |
| [`trunk-proxy-route-conflict`](trunk-proxy-route-conflict.md) | trunk panic, `__private__axum_nest_tail_param`, `conflict with previously registered route`, silent tmux pane close | workaround documented |
| [`wasm-getrandom-unresolved-imp`](wasm-getrandom-unresolved-imp.md) | `unresolved module imp`, getrandom, wasm32-unknown-unknown, E0433 | workaround documented |
| [`webrtc-mdns-lan-ap-isolation`](webrtc-mdns-lan-ap-isolation.md) | iceConnectionState Checking → Failed / Disconnected, WebRTC DataChannel never opens, two browsers on same WiFi can't connect, mDNS `<uuid>.local` candidate not resolvable for WebRTC, signalingState Stable but no data flow, AP isolation hypothesis ruled out, general mDNS works (`_airplay._tcp` / `_miio._udp` visible) but WebRTC mDNS specifically fails, Xiaomi AX9000 / MiWiFi firmware, iPhone hotspot fixes it | hypothesis-still-open, workaround documented |

## Cross-referenced pitfalls (still in their original homes)

These traps are documented elsewhere and aren't duplicated here — the table
exists so grepping `pitfalls/` still finds them. Move into this folder only
if their original location stops being a natural reading flow.

| Trap | Lives in | Why not here |
|---|---|---|
| (example: Tool X version Y bug) | `docs/tool-x.md` → "Known issues" | Already part of the tool's normal config narrative |
