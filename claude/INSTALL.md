# Wiring contour into Claude Code

Deliberately outside the skill, so the skill stays a copy of one file.

## 1. Install the binary

```sh
brew install dpep/tools/contour
```

That is the one to want: it builds `semantic-dynamic` against the `onnxruntime`
keg, so the real embedder is on by default. From source, pick the feature
yourself — the crate is published as `contour-index`, because `contour` was
taken; the binary is still `contour`.

```sh
cargo install contour-index --features semantic
# or, against a system onnxruntime (smaller, no build-time download):
brew install onnxruntime && cargo install contour-index --features semantic-dynamic
```

**From cargo, install it with a feature, and reinstall it with the same one.**
Without
either, contour still runs, using a deterministic hash embedder that exercises
the whole pipeline but is not a trained model — English search then matches what
code is *called*, not what it does, which is most of the tool.

```sh
cargo install --path .       # fine for hacking on contour; not what you want installed
```

The two builds are indistinguishable on disk, and `cargo install` takes whatever
features the command line gives it rather than what is already there — so a
routine reinstall with the flag left off silently downgrades a working index to
name matching. `contour --version` says which one you have, and so does
`contour --status`; `contour search` says which one actually answered, which can
differ for a `semantic-dynamic` build if no system ONNX Runtime is found.

## 2. Index once per checkout

```sh
cd ~/code/your-app && contour index
```

Facts are keyed by git blob OID, so every worktree of a repo shares one index
and a reindex with no edits parses nothing.

## 3. Register the MCP server

```sh
claude mcp add contour -- contour mcp
```

It speaks MCP over stdio and exposes `search`, `similar`, `dupes`, `symbols`,
`status`, `pending`, `store_summary`, and `index`. The two write-adjacent tools
(`pending` and `store_summary`) are what let a session feed the index as it
works — see the skill's "Feeding the index".

## 4. Install the skill

Copy `claude/contour-skill.md` to `~/.claude/skills/contour/SKILL.md`.
