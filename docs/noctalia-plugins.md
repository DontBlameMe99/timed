# Noctalia plugin API docs

This plugin targets the Noctalia plugin API (Luau entry scripts plus a
`plugin.toml` manifest). The authoritative documentation is online at
docs.noctalia.dev, so read it there instead of guessing at an API from this
repo. The pages below are the ones worth fetching first.

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
