# mtga-reader — working notes

Native library that reads Magic: The Gathering Arena's game memory, exposed to
Node via napi-rs. Two runtimes, one API:

- **Windows / Linux (Wine) → Mono** — `src/mono/`, `src/queries.rs`, `src/session.rs`
- **macOS → IL2CPP** — `src/il2cpp/`, `src/queries_il2cpp.rs`, `src/session_il2cpp.rs`

Both return **identical JSON shapes**, so callers never branch on platform. If you
change a reader's output on one side, change the other.

Read [docs/MACOS_IL2CPP.md](docs/MACOS_IL2CPP.md) before touching anything macOS —
it documents the verified struct layout, the object graph, and the offsets that
fail *silently* when wrong.

## Building and testing

**Never run `sudo cargo …`.** It leaves root-owned files in `target/`, and later
user-level builds then fail with a misleading `ld: can't write output file`.
Build as your user, run the binary elevated:

```bash
cargo build --bin probe_il2cpp
sudo ./target/debug/probe_il2cpp
```

(If you hit that link error anyway: `find target -user root -delete`, then
`rm -rf` the leftover root-owned directories.)

Requires MTGA running, on the **home screen** for the typed readers — the wrapper
scene is unloaded during a match. macOS additionally needs root *or* the
`com.apple.security.cs.debugger` entitlement; see the docs.

The napi addon only builds as a **lib** (`--cargo-flags="--lib"` in the npm
scripts). `cargo build --bins --features napi-bindings` fails to link because the
napi symbols come from the Node host — that's expected, not a regression.

`cargo build --bins` on macOS also fails on `http_server_simple`, which is the
Mono/Windows debug server. Pre-existing; build the specific binary you need.

## Platform gating

macOS-only modules are `#[cfg(target_os = "macos")]` in `src/lib.rs`, and the
macOS binaries carry a non-macOS stub `main`. CI builds four targets, so before
pushing verify more than your own host:

```bash
cargo check --lib --features napi-bindings --target x86_64-unknown-linux-gnu
cargo check --lib --features napi-bindings --target x86_64-pc-windows-msvc
```

## Releasing

CI publishes to npm when the **last commit message is a bare `x.y.z`**, so follow
the documented flow: descriptive commit, then `npm version patch|minor|major`
(which creates that commit plus an annotated tag), then
`git push --follow-tags`. All platform `.node` binaries ship inside the *single*
main package — the `npm/<triple>/` directories are vestigial and nothing publishes
them.

`npm test` gates the publish job. **ava's default glob picks up `test-*.js` at the
repo root**, so a stray helper script there fails the run and silently blocks the
release. Dev helpers live in `scripts/` (see `scripts/macos-smoke.js`).

Consumers pin via lockfiles, so publishing is not enough — a dependent repo also
needs its `package-lock.json` updated to actually pick the new version up.

## Tools

| tool | use |
|---|---|
| `src/bin/probe_il2cpp.rs` | macOS discovery/verification: dumps the detected layout, root singleton, and the full field table at every hop. **Run this first after a game update.** |
| `src/bin/test_readers_macos.rs` | End-to-end typed readers with timings |
| `src/bin/http_server_il2cpp.rs` | macOS debug server for `debug-ui` (mirrors the Mono server's routes) |
| `src/bin/http_server_simple.rs` | Mono/Windows equivalent |
| `scripts/macos-smoke.js` | Same checks through the built `.node` addon |

`/singletons` on the debug server lists every class whose statics hold a live
instance — that's how you find a new root if the game moves one.

## Gotchas worth knowing

- **Typed readers are home-screen only** on Mono. On IL2CPP the root falls back to
  `PAPA._instance`, so account/inventory/collection keep working mid-match; ranks
  and decks still don't.
- **`percentile` is a float on current builds**, not a string. It's read
  type-aware, so either works.
- **Field names track the C# source**, so they're shared across platforms — but the
  *game* changes them. `CommanderGrpId` no longer exists on `PlayerInfo`; it is now
  `<Commanders>k__BackingField` (a `List<T>`). That affects Windows too.
