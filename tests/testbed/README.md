# The testbed

Corner cases we have already paid for, in a form where recording the next one
costs nothing.

**Adding a case is dropping in files. No Rust.** `tests/testbed.rs` iterates
every directory here; a new one is picked up automatically.

```text
tests/testbed/003-your-case/
  app.rb        one or more Ruby files — a tiny source tree
  expected      one assertion per line
  history       optional: which files get their own commit, oldest first
```

Each case is staged as a real git checkout with its own database and indexed,
so it exercises the whole path: extract → store → CLI.

## The `expected` format

```text
# Why this case exists. Say what broke, not what the code does.
symbols app.rb  Widget#save,Widget.find,Widget#name
dupes           Widget#save+Gadget#store  Alpha#run+Beta#run
```

| verb | asserts |
| ---- | ------- |
| `symbols FILE  a,b,c` | the outline of `FILE`, in source order, as `Owner#name` / `Owner.name` |
| `dupes  a+b  c+d` | the **whole** clone report: groups space-separated, members `+`-joined. `(none)` for an empty report. |
| `canonical  a+b  a` | which member of that group the signals name as the original, or `(none)` when they decline to |

`dupes` asserts the entire report, not just the groups you care about — "a
rename collides" is half a claim without "changed logic does not", so every
case carries its own control. Ids and groups are both sorted before comparing,
so a case pins what hashes together and not how the report ranks it.

The harness runs `dupes` with `--min-lines 1` so fixtures can stay tiny. The
default floor is a usability knob measured on a real corpus and is pinned in
`tests/cli_e2e.rs` instead.

## `history`, and why every commit is dated

Without a `history` file a case is one commit, so every body in it is the same
age — which is a fixture worth having, because it is what makes the git-age
signal tie. With one, each named path gets its own commit a year apart, oldest
first, and anything unnamed lands in the first. That is what lets a case say
*this* body came before *that* one.

Every commit is dated explicitly, never with the wall clock: `git blame` is a
canonicality signal, so when a body was written has to be a property of the
fixture rather than of the machine running the suite.

`canonical` expectations are hermetic in one more way — the harness points
`$CONTOUR_TREKR` at a path that does not exist, so no case consults whatever
trekr this machine has or writes a temp repo into its global index. The
reference signal reports itself absent, which is a degraded path worth pinning
in its own right.

Comment lines start with `#`; a `#` inside an expectation (`Widget#save`) is
just a character.

An unknown verb fails loudly: a typo in an expectation is a test that proves
nothing.

## Writing a good case

- **Make it fail first.** Check every case against a build with the fix
  removed. A case that passes both ways is worse than no case.
- **Pin behaviour, not wishes.** If the current answer is imperfect but
  deliberate, record it and say so in the comment — then a change to it is a
  decision rather than a surprise.
- **Keep the source tiny and generic** — `Widget`, `Job`, `Alpha`. Public repo.
