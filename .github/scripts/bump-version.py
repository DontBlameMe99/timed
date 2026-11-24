#!/usr/bin/env python3
"""Bump the plugin version in Cargo.toml and catalog.toml."""

import re
import sys
from pathlib import Path

BUMP = sys.argv[1]
FILES = (Path("Cargo.toml"), Path("catalog.toml"))
PATTERN = re.compile(r'^version = "(\d+)\.(\d+)\.(\d+)"$', re.MULTILINE)

if BUMP not in ("major", "minor", "patch"):
    raise SystemExit(f"unknown bump: {BUMP}")

matches = {path: PATTERN.search(path.read_text()) for path in FILES}
if any(match is None for match in matches.values()):
    raise SystemExit("version line not found in Cargo.toml or catalog.toml")

versions = {(m.group(1), m.group(2), m.group(3)) for m in matches.values() if m}
if len(versions) != 1:
    raise SystemExit("Cargo.toml and catalog.toml versions differ")

major, minor, patch = (int(part) for part in versions.pop())
if BUMP == "major":
    major, minor, patch = major + 1, 0, 0
elif BUMP == "minor":
    minor, patch = minor + 1, 0
else:
    patch += 1

new = f"{major}.{minor}.{patch}"
for path in FILES:
    path.write_text(PATTERN.sub(f'version = "{new}"', path.read_text(), count=1))

print(new)
