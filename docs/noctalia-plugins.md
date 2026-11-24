# Noctalia plugin API docs

The plugin is written in Rust with
[noctalia-rs](https://github.com/lumi673/noctalia-rs), which generates the Luau
entry stubs and the `plugin.toml` manifest. The host API is still the Noctalia
plugin API. The authoritative documentation is online at docs.noctalia.dev, so
read it there instead of guessing at an API from this repo. The pages below are
the ones worth fetching first.

- [Plugin development overview](https://docs.noctalia.dev/noctalia/plugins/development/)
- [Manifest and settings](https://docs.noctalia.dev/noctalia/plugins/development/manifest/)
- [Entry scripts](https://docs.noctalia.dev/noctalia/plugins/development/entries/)
- [Bar widgets](https://docs.noctalia.dev/noctalia/plugins/development/entries/#barwidget---widget-presentation)
- [Declarative UI](https://docs.noctalia.dev/noctalia/plugins/development/declarative-ui/)
- [Runtime API](https://docs.noctalia.dev/noctalia/plugins/development/runtime-api/)
- [Plugin API versions](https://docs.noctalia.dev/noctalia/plugins/development/plugin-api/)
- [Official plugins repo](https://github.com/noctalia-dev/official-plugins)
- [noctalia.d.luau](https://github.com/noctalia-dev/official-plugins/blob/main/noctalia.d.luau)
- [noctalia-shell source](https://github.com/noctalia-dev/noctalia)

`noctalia.d.luau` in the official plugins repo is the exact type definition for
globals such as `noctalia.*`, `ui.*`, `barWidget.*`, `panel.*`, and
`launcher.*`.
