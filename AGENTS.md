# Timed

Noctalia plugin written in Rust with
[noctalia-rs](https://github.com/lumi673/noctalia-rs). The plugin logic lives in
`src/`. `timed/` is the directory Noctalia loads: generated entry stubs, the
shared bridge, `plugin.toml`, translations, assets, and the static helper.

`noctalia.toml` is the manifest source. `timed/plugin.toml` and
`catalog.toml` must match, so metadata and settings live in `noctalia.toml` and
`catalog.toml`, and `timed/` is regenerated rather than edited.

## Checks

CI runs these:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release
python3 .github/scripts/check-catalog.py
cargo deny check
cargo run --release -- emit "$TMP/plugin" timed-helper
diff -r --exclude=timed-helper "$TMP/plugin" timed
luau bench/luau/tests/duration_test.luau
```

`nix flake check` runs the build, clippy, test, and format checks in one go.

The lint, format, and dependency rules are copied from
`~/projects/project_templates/rust`. `src/` and `tests/` follow the strict
workspace lints, including checked arithmetic and no panics outside tests. The
`bench/` crate is a dev harness and declares an empty `[lints]` table instead,
so it keeps the default set. Format with stable rustfmt: the nightly-only keys
in `.rustfmt.toml` are ignored, and CI uses stable.

The plugin is GPL-3.0-or-later because the helper links noctalia-rs. Keep
`catalog.toml` and the workspace license in sync.

Regenerate the plugin directory after changing `noctalia.toml`, `src/`, the
translations, or the assets:

```sh
cargo run --release -- emit timed timed-helper
```

The committed `timed/timed-helper` is a static x86_64 build. The release
workflow rebuilds it. Do not edit anything under `timed/` by hand.

The Luau LSP step needs the definitions file that CI downloads. See
`.github/workflows/ci.yml`. `luau` and `luau-lsp` are not installed here.

## Release

`workflow_dispatch` on `.github/workflows/release.yml` is the only release
path. It bumps `Cargo.toml` and `catalog.toml`, runs the checks, builds the
static musl helper, regenerates `timed/` from the bumped source, commits both,
tags, and publishes. `dry_run=true` stops before any push.

`timed/timed-helper` must always be the static build of the committed source.
The workflow replaces it, and CI fails when the committed file is dynamically
linked. The release also needs the `RELEASE_DEPLOY_KEY` secret.

Regenerating `timed/` with the host helper is fine for manifest or handler
changes, because `emit` only writes the manifest, stubs, bridge,
translations, and assets:

```sh
cargo run --release -- emit timed timed-helper
```

A version bump or a release also needs the static helper. Build the musl target
and copy that binary, as the workflow does:

```sh
python3 .github/scripts/bump-version.py patch
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
  CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C link-self-contained=yes" \
  cargo build --release --target x86_64-unknown-linux-musl
./target/x86_64-unknown-linux-musl/release/timed emit timed timed-helper
cp target/x86_64-unknown-linux-musl/release/timed timed/timed-helper
python3 .github/scripts/check-catalog.py
```

## Benchmark

`bench/` holds the original Luau implementation, unmodified, and a paired
benchmark. Both sides run the same fixture and must emit the same ops before
timing is reported.

```sh
cargo run --release -p timed-bench -- --samples 200 --warmup 20 --batch 10 \
  --helper timed/timed-helper
```

It needs `luau` on `PATH` (or `--luau <path>`).

## API

`docs/noctalia-plugins.md` links the API pages and `noctalia.d.luau`. In Rust,
read the `noctalia_rs` crate (`Context`, `Op`, `ui`, `Setting`, `EntryConfig`)
instead of guessing at the host surface. The generated bridge is the only Luau
in the plugin directory, and `emit` owns it.

## Python

PyYAML is not installed. Read `catalog.toml` and `timed/plugin.toml` with
`tomllib`. Do not parse YAML with Python.
