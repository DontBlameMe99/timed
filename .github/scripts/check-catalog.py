#!/usr/bin/env python3
"""Fail when catalog.toml disagrees with a plugin manifest."""

import sys
import tomllib
from pathlib import Path

FIELDS = (
    "id",
    "name",
    "version",
    "plugin_api",
    "author",
    "license",
    "dependencies",
    "tags",
    "icon",
    "description",
    "deprecated",
)

MISSING = object()

DEFAULTS = {
    "deprecated": False,
    "dependencies": [],
    "license": "MIT",
    "tags": [],
}


def load(path):
    with path.open("rb") as handle:
        return tomllib.load(handle)


def main(root):
    catalog_path = root / "catalog.toml"
    if not catalog_path.is_file():
        print(f"error: {catalog_path} is missing", file=sys.stderr)
        return 1

    entries = {}
    for entry in load(catalog_path).get("plugin", []):
        entries.setdefault(entry.get("id"), []).append(entry)

    errors = []
    manifests = sorted(root.glob("*/plugin.toml"))
    catalogued = set()

    for manifest_path in manifests:
        manifest = load(manifest_path)
        plugin_id = manifest.get("id")
        matches = entries.get(plugin_id, [])
        if len(matches) != 1:
            errors.append(f"catalog.toml has {len(matches)} entries for {plugin_id!r}")
            continue
        catalogued.add(plugin_id)
        for field in FIELDS:
            value = manifest.get(field, DEFAULTS.get(field, MISSING))
            if field not in matches[0]:
                errors.append(f"catalog.toml entry {plugin_id}: missing {field!r}")
            elif value is MISSING:
                errors.append(f"{manifest_path}: missing {field!r}")
            elif matches[0][field] != value:
                errors.append(
                    f"{plugin_id}: {field} is {matches[0][field]!r} in catalog.toml "
                    f"but {value!r} in {manifest_path}"
                )

    for plugin_id in entries:
        if plugin_id not in catalogued:
            errors.append(f"catalog.toml entry {plugin_id!r} has no plugin manifest")

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    print(f"Catalog matches {len(manifests)} plugin manifest(s).")
    return 0


if __name__ == "__main__":
    sys.exit(main(Path(sys.argv[1] if len(sys.argv) > 1 else ".")))
