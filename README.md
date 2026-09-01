# solenv

A **project-local** Solana development environment manager, inspired by Python's
`venv` and by `asdf`/`nvm`—but scoped to the Solana ecosystem. Every project gets
its own pinned toolchain (Rust, Solana/Agave CLI, Anchor, Node) isolated in a
`.solenv/` directory, so switching between projects never fights over global
tool versions.

```

  .solenv/
  ├── bin/          # version-pinned binaries (resolved for `solenv run`)
  ├── versions/     # installed toolchain versions  (tool / version / ...)
  ├── cache/        # downloaded artifacts + checksums
  └── state.toml    # what is currently installed/active
```

## Why?

The Solana + Anchor toolchain has notoriously fragile version coupling:

- Anchor releases are built against a **specific** Solana CLI and a **specific**
  Rust version, and the crates they depend on track a particular Solana crate
  line (v1 vs v2).
- Node's major version matters for tooling like `@kolomix/solana` and bundlers.

`solenv` encodes that coupling in a versioned **compatibility matrix**, validates
your pinned set *before* you build, and pins every tool inside the project so a
team shares one reproducible environment.

## Highlights

- **Project-scoped.** Everything lives in `.solenv/`. Nothing touches your global
  toolchains (unless you ask via `dev-cli` installers).
- **Compatibility-aware.** Sees a `[toolchain]` block, validates it against a
  Solana Foundation-derived compatibility dataset, and refuses obviously broken
  combinations.
- **Checksum-verified.** Downloads are verified against official digests before
  they are installed.
- **No shell string execution.** All tool orchestration uses `std::process::Command`
  with explicit argv arrays.
- **Friendly UX.** Actionable errors, progress output, a `doctor` command for
  diagnosing the machine, and non-interactive (`--yes`) modes for CI.

## Installation

> **Requirements:** macOS or Linux, `bash`/`zsh`. `rustup` and `node` are used by
> the `install`/`init` commands as bootstraps; `curl`/`tar` are used by downloaders.

Build from source:

```sh
cargo build --release
# binary at target/release/solenv
```

Or install via cargo:

```sh
cargo install --path .
```

## Quick start

```sh
# 1. From the root of a Solana/Anchor project, detect your current toolchain.
solenv init

# 2. Review the generated solenv.toml, tweak pins if needed, then install.
solenv install

# 3. Verify the pinned set is mutually compatible and present.
solenv check

# 4. Run commands through the pinned toolchain.
solenv run solana --version
solenv run anchor build
```

Existing `Anchor.toml` `[toolchain]` blocks are picked up by `init` so migration is
drop-in.

## `solenv.toml`

```toml
[toolchain]
rust = "1.92.0"          # pinned via rustup (RUSTUP_TOOLCHAIN)
solana = "3.1.10"        # solana-release-<triple>.tar.bz2 from agave releases
anchor = "1.1.2"         # prebuilt anchor-<ver>-<triple> from solana-foundation/anchor
node = "24"              # any version resolution is fine; use a major for "latest patch"
package_manager = "npm"  # npm | pnpm | yarn | bun | npx | corepack
```

Version syntax supports exact (`1.92.0`), partial (major-only like `24`), and
wildcard pins (`3.x`, `3.1.x`). Any non-exact pin is resolved to a concrete
published version before install: major-only Node (`24`) resolves against the
Node dist index, `3.0.x` style Solana/Anchor pins resolve against the latest
published GitHub release tag matching the pattern, and Rust `1.91.x` resolves
to the best matching installed rustup toolchain. `solenv check`, `install`,
`run`, `list`, and `doctor` all use the same resolution, so a wildcard pin is
reported and run as the exact version it resolves to.

## Commands

| Command | Description |
| --- | --- |
| `solenv init [--yes] [--set tool=ver ...]` | Create `solenv.toml`, detecting the current toolchain and an existing `Anchor.toml`. |
| `solenv install [--force] [--only rust,solana,...]` | Install the pinned toolchain into `.solenv/`. Skips batch if missing; validates compatibility first. |
| `solenv check [--declared-only]` | Validate the pinned set against the compatibility matrix and report installed state. |
| `solenv list` | Show configured + installed toolchain versions. |
| `solenv run <command> ...` | Run `<command>` with `.solenv`'s pinned binaries prepended to `PATH`. |
| `solenv doctor` | Diagnose common environment problems (rustup, node, compatibility dataset). |
| `solenv clean [--cache] [--yes]` | Remove installed toolchain versions (optionally cached downloads too); keeps `solenv.toml`. |
| `solenv uninstall [--yes]` | Remove `.solenv/` for the project; keeps `solenv.toml`. |
| `solenv --dir <root>` | Run any command treating `<root>` as the project root (config discovery walks upward). |
| `solenv --quiet` | Suppress progress output. |

## How installation works

Details are isolated behind a small manager interface per tool:

- **Rust** — orchestrated through `rustup` (`rustup toolchain install <ver>`,
  `RUSTUP_TOOLCHAIN=<ver>` pinning). Does not modify the default toolchain.
- **Solana / Agave CLI** — downloads `solana-release-<triple>.tar.bz2` from
  `anza-xyz/agave` releases; SHA-256 verified against the GitHub release digest,
  then extracted from the bzip2 tarball into `.solenv/versions/solana/<ver>/`.
- **Anchor** — downloads the prebuilt `anchor-<ver>-<triple>` release binary from
  `solana-foundation/anchor`; SHA-256 verified against the GitHub release digest.
- **Node** — downloads `node-v<ver>-<os>-<arch>.tar.xz` from `nodejs.org/dist`,
  verified against `SHASUMS256.txt`. A major-only pin (e.g. `24`) is resolved
  through `https://nodejs.org/dist/index.json`.
- **Package manager** — provisioned via `corepack` (npm/yarn/pnpm) or the global
  npm fallback; never run through a shell.

All downloads are HTTPS-only and served into `.solenv/cache/`.

## Compatibility validation

`src/../data/compatibility.toml` (embedded at build time) is the source of truth.
It is derived from the [Solana Foundation
compatibility matrix](https://github.com/solana-foundation/solana-dev-skill/blob/main/skills/solana-dev/references/compatibility-matrix.md)
and captures:

- Anchor ↔ Solana CLI pairings (with min/max bounds).
- Anchor ↔ Rust and Anchor ↔ Node pairings.
- Recommended Solana **platform-tools** versions.
- Known-good combination notes.

Rules are **first-match-wins**, so the most specific entry is honored over the
`*` catch-all. `solenv check` and `solenv install` surface a clear
`Incompatible toolchain` diagnostic listing the offending versions and the
compatible ranges when a pin falls outside them.

## Security model

- Downloads are restricted to `https://`.
- Every artifact is verified against an official digest (GitHub API `sha256` for
  Solana/Anchor, `SHASUMS256.txt` for Node) before installing.
- No tool is ever run through a shell string; argv is passed as an array.
- Installation is project-local; global toolchains are only ever *read* (rustup
  inspection) and not modified unless a chosen installer path requires it.
- Running as root is refused.

## Limitations / roadmap

- macOS/Linux only for this MVP (no Windows).
- `bash`/`zsh` shell environments (no PowerShell, limited fish/csh support).
- The compatibility matrix is a best-effort encoding of upstream documents, not an
  official API; treat it as advisory and update `data/compatibility.toml` as the
  ecosystem moves.
- Node installs a full runtime; a lightweight per-project bootstrap is a future
  optimization.

## Development

```sh
cargo build       # 0 warnings expected
cargo test        # unit + integration (53 + 12) — no network required
cargo clippy      # lint
```

## License

Licensed under the Apache License, Version 2.0 (the "License"); you may not use
this project except in compliance with the License. You may obtain a copy of the
License at <https://www.apache.org/licenses/LICENSE-2.0>.

Unless required by applicable law or agreed to in writing, software distributed
under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
CONDITIONS OF ANY KIND, either express or implied. See the License for the
specific language governing permissions and limitations under the License.
