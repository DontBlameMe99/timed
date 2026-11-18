#!/usr/bin/env python3
"""Print the community-plugins PR body for an automated update."""

import os

summary = os.environ["SUMMARY"]
tested_on = os.environ["TESTED_ON"]
noctalia_version = os.environ["NOCTALIA_VERSION"]
api = os.environ["API"]
branch = os.environ["BRANCH"]


def check(compositor: str) -> str:
    return "x" if tested_on == compositor else " "


print(
    f"""<!-- noctalia-pr-template:v1 -->

## Plugin

- **Id:** `dontblameme/timed`
- [ ] New plugin
- [x] Update to an existing plugin (version bumped in `plugin.toml`)

## What it does

{summary}

## External dependencies

None. Completion notifications call `noctalia.notify(title, body)` and follow the
shell's notification timeout. The plugin writes `timers.json` in Noctalia's
per-plugin data directory and makes no network calls.

## Testing

- Exercised the launcher and the service entries with this version.
- [{check("Hyprland")}] Tested on Hyprland
- [{check("Niri")}] Tested on Niri
- [{check("Sway")}] Tested on Sway
- **Noctalia version tested against:** {noctalia_version}
- **Plugin API level:** {api}

## Screenshots / Videos

![Starting a timer from the launcher](https://raw.githubusercontent.com/DontBlameMeDev/community-plugins/{branch}/timed/screenshot.webp)

## Checklist

- [x] The directory name matches the part of `id` after the `/` in `plugin.toml` exactly.
- [x] It ships `plugin.toml`, `README.md`, `thumbnail.webp`, and `translations/en.json`.
- [x] `README.md` follows the [README template](https://github.com/noctalia-dev/community-plugins/blob/main/README_TEMPLATE.md), documents every entry id and dependency, and includes exact panel IPC commands and launcher prefixes where applicable.
- [x] `thumbnail.webp` is present and relevant; for a new plugin I created it with the [thumbnail generator](https://assets.noctalia.dev/plugins/thumbnail-generator.html), and for an update I regenerated it with the generator if the visual identity or user-facing appearance changed.
- [x] `version` follows semver and is bumped in this PR; `plugin_api` is the oldest API level this plugin requires.
- [x] Every non-English translation in this PR uses a locale supported by Noctalia core, and I can read, write, and understand that language well enough to review and maintain it (no unreviewed machine/LLM translations).
- [x] I did not edit `catalog.toml`; CI generates it.
- [x] This PR touches exactly one plugin directory.

## Code review attestation

- [x] The code is readable and not obfuscated, minified, or generated.
- [x] Code identifiers and code comments are in English.
- [x] It does not download and execute remote code.
- [x] Every network call, filesystem write, and spawned process is something the description above accounts for.
- [x] I have the right to publish this code under the `license` declared in `plugin.toml`."""
)
