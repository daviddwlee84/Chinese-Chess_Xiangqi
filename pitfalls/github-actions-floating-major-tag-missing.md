# `Unable to resolve action <owner>/<repo>@vN` after bumping a major

**Symptom**

A workflow that previously ran fine fails immediately at "Set up job" after a routine major-version bump:

```
##[error]Unable to resolve action `actions/configure-pages@v7`,
unable to find version `v7`
```

The job never reaches any user step. `gh api repos/<owner>/<repo>/releases/latest` confirms the version exists:

```sh
$ gh api repos/actions/configure-pages/releases/latest -q .tag_name
v7.0.1
```

…yet the workflow reference (`@v7`) refuses to resolve.

## Root cause

GitHub Actions resolves a `uses: <owner>/<repo>@<ref>` reference against **git refs** (branches and tags) on the action's repository, not against GitHub Release objects. Many — but not all — official action repositories maintain a **floating major tag** (`v7`, updated to point at the latest `v7.x.y`) in addition to the precise release tags (`v7.0.0`, `v7.0.1`, …).

When a maintainer publishes the first `v7.0.0` release without also force-pushing the `v7` floating tag, the situation is:

| Ref | Exists? |
|---|---|
| `v7.0.0` tag | ✅ |
| `v7.0.1` tag | ✅ |
| `v7` floating tag | ❌ (until the maintainer publishes it) |
| GitHub release object `v7.0.1` | ✅ (this is what `releases/latest` returns) |

So `releases/latest` is **misleading** for action pinning — it tells you what shipped, not what `@vN` will resolve.

## Diagnostic

Always check the **tags** endpoint, not releases:

```sh
gh api repos/<owner>/<repo>/tags --jq '.[].name' | head -20
```

Real example from 2026-05-08:

```
$ gh api repos/actions/configure-pages/tags --jq '.[].name' | head -10
v6.0.0
v6           ← floating major exists for v6
v5.0.0
v5
v4.0.0
v4
                    ← no v7 entry, even though v7.0.1 release exists
```

vs.

```
$ gh api repos/actions/upload-pages-artifact/tags --jq '.[].name' | head -10
v5.0.0
v5           ← floating major exists for v5 → @v5 resolves
v4.0.0
v4
```

## Fix

Pick the highest floating major that **actually appears as a tag**, not the highest release. In the 2026-05-08 incident:

```diff
- - uses: actions/configure-pages@v7   # release v7.0.1 exists, but no v7 tag yet
+ - uses: actions/configure-pages@v6   # latest floating major that resolves
```

When the maintainer eventually publishes the `v7` floating tag (typically days to weeks after the first `v7.x.y` release), the bump can be redone.

## Alternatives if you must use the unreleased major right now

- Pin to the exact release tag: `actions/configure-pages@v7.0.1`. Loses auto-patch updates but unblocks immediately.
- Pin to the commit SHA: `actions/configure-pages@<40-char-sha>`. Also what Dependabot's "pin to SHA" policy produces; most reproducible, ugliest diff.

## Why this bites in deprecation-warning sweeps

The "Node.js 20 actions are deprecated" annotations from late 2025 / early 2026 push everyone to bulk-bump their pages / artifact / checkout actions at the same time. That bulk bump tends to land **before** every action repo has caught up on republishing floating major tags, so several of them will fail this way during a single bump session. Always do the `gh api .../tags` check before pushing the bump commit.

## Related

- Session that surfaced this: 2026-05-08, commit `94dc88e` failed at job setup with `@v7`, recovered in `e573c72` by pinning to `@v6`.
- Pattern applies to **any** GitHub Action, not just `actions/*` — community actions are even more likely to skip publishing the floating major tag.
