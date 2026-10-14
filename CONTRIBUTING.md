# Contributing

Thanks for looking. Issues, bug reports and pull requests are all welcome — in
English or in French, whichever you are comfortable with.

## Getting it to build

```bash
git clone https://github.com/Sebdev43/KaraokeTyper.git
cd KaraokeTyper
npm install
npm run tauri dev
```

On Linux, a GPU build (`--features gpu-vulkan`, what the released binaries ship)
compiles ggml's Vulkan backend from source and needs three system packages:

```bash
sudo apt install libvulkan-dev glslc spirv-headers
```

A plain `cargo build` is CPU-only and needs none of them. macOS uses Metal
(`--features gpu-metal`), Windows uses the Vulkan SDK.

Everything else — Rust, Node, the models — is handled by `npm install` and by the
app itself, which downloads models on demand into your user data directory.

## Before you push

```bash
git config core.hooksPath scripts/git-hooks
```

That installs a `pre-push` hook running the same three gates as CI, in about
thirty seconds. It is versioned in the repository, so this one command is all
each clone needs. Use `git push --no-verify` when you deliberately want to skip
it.

If you prefer to run them by hand:

```bash
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

and, for the frontend:

```bash
npm run lint
npm run build
```

CI runs all of these, plus a `cargo check --features gpu-vulkan`, plus a weekly
job that re-verifies every model hash in the catalogue against its upstream
repository.

## Branches

`dev` is where work lands; `master` carries the releases. Open your pull request
against **`dev`**. Releases are tagged on `master` as `vX.Y.Z`, which is what
triggers the Linux, Windows and macOS builds.

## Commits

One coherent change per commit. The message explains **why**, not what the diff
already shows — what broke, what the old behaviour cost, why this approach.
Prefixes follow [Conventional Commits](https://www.conventionalcommits.org):
`fix:`, `feat:`, `refactor:`, `docs:`, `chore:`, `build:`, `ci:`.

## Code

- No `.unwrap()` on a production path. `?` or `.expect("why")` for a genuine
  invariant.
- A test must be able to fail. Before trusting a new one, remove the fix and
  watch it go red.
- Comments explain the reasoning a reader cannot recover from the code. A
  comment restating the line below it is noise.
- Clippy runs with `-D warnings`. An `#[allow]` needs a comment saying why.

## Models

`src-tauri/src/catalog/catalog.json` pins every model by SHA-256. Never edit a
hash by hand: `scripts/catalog.py check` verifies them against upstream, and
`scripts/catalog.py update` rewrites them. A download whose hash does not match
is refused, which is the point.
