# Timed

![Timed](timed/thumbnail.webp)

A Noctalia plugin for countdown timers started from the launcher.

## Install

Once the plugin is listed in the Noctalia plugin store, install it there. To
use this repository as a plugin source instead:

```sh
noctalia msg plugins source add timed git https://github.com/DontBlameMeDev/timed.git
noctalia msg plugins enable dontblameme/timed
```

See [timed/README.md](timed/README.md) for usage and duration formats.

## Release

The first version is submitted to the community store by hand. After it is
merged, run the **Release** workflow and pick `patch`, `minor`, or `major`. It
bumps the version in `timed/plugin.toml` and `catalog.toml`, then runs
**Publish to Noctalia store**, which opens or updates the PR against
`noctalia-dev/community-plugins`.

The publish workflow uses the `COMMUNITY_PLUGINS_TOKEN` repository secret: a
classic personal access token with `public_repo` scope.
