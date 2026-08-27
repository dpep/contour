# Wiring contour into Claude Code

Deliberately outside the skill, so the skill stays a copy of one file.

## 1. Install the binary

```sh
cargo install --path .
```

For real semantic search rather than the hash fallback, build with the ONNX
embedder:

```sh
cargo install --path . --features semantic
# or, against a system onnxruntime (smaller, no build-time download):
brew install onnxruntime && cargo install --path . --features semantic-dynamic
```

Without either feature contour still runs, using a deterministic hash embedder
that exercises the whole pipeline but is not a trained model. `contour search`
discloses which embedder answered, so you always know which you have.

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
