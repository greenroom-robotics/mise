#!/usr/bin/env python3
"""Merge the ros-recipes pixi channel grants into ~/.pixi/config.toml.

Only the `azure-options` key is replaced; every other key, comment and
formatting in an existing config is preserved. Container names are masked
before anything else is printed, since logs of public repos are public.
"""

import os
import pathlib
import subprocess
import sys
import tempfile

import tomlkit

GRANTS = "repos/greenroom-robotics/ros-recipes/contents/ci/azure-options.toml"
CONFIG = pathlib.Path.home() / ".pixi" / "config.toml"


def fetch_grants() -> tomlkit.TOMLDocument:
    result = subprocess.run(
        ["gh", "api", "-H", "Accept: application/vnd.github.raw", GRANTS],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        sys.exit(
            "::error::could not fetch pixi channel grants; "
            "does the GitHub App have read access to ros-recipes?"
        )
    return tomlkit.parse(result.stdout)


def main() -> None:
    options = fetch_grants()["azure-options"]
    for account in options.values():
        for container in account["auth"]:
            print(f"::add-mask::{container}")

    config = tomlkit.parse(CONFIG.read_text()) if CONFIG.exists() else tomlkit.document()
    config["azure-options"] = options

    CONFIG.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", dir=CONFIG.parent, delete=False) as tmp:
        tmp.write(tomlkit.dumps(config))
    os.replace(tmp.name, CONFIG)


if __name__ == "__main__":
    main()
