# Timed

Noctalia plugin. Luau plus `timed/plugin.toml`. No build step and no dev server.

## Checks

CI runs these:

```sh
python3 .github/scripts/check-catalog.py
luau tests/duration_test.luau
```

The Luau LSP step needs the definitions file that CI downloads. See
`.github/workflows/ci.yml`. `luau` and `luau-lsp` are not installed here, so
rely on CI unless you install them.

## API

`docs/noctalia-plugins.md` links the API pages and `noctalia.d.luau`. Read them
instead of guessing at `noctalia.*`, `ui.*`, `barWidget.*`, `panel.*`, or
`launcher.*`.

## Python

PyYAML is not installed. Use `tomllib` from the standard library.
