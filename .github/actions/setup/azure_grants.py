#!/usr/bin/env python3
"""Turn a list of az:// channel URLs into per-container pixi credential grants.

pixi 0.75+ signs a request to an Azure Blob channel only if the container has an
explicit grant. Without one the request goes out UNSIGNED and a private container
answers 401 — the ambient `az login` is not enough on its own.

Grants deliberately cannot live in a repo's own `.pixi/config.toml`: pixi drops
the whole `azure-options` table from project-scoped config, so that cloning a
repo can never make your credentials sign requests to a host the repo names. They
have to be written to the user-scope config, which is what this does. One path
covers pixi and rattler-build both, since rattler-build's config discovery reads
pixi's user config as well as its own.

Reads AUTH_CHANNELS (newline- or space-separated az:// URLs) from the env rather
than argv so the values never appear in a rendered command line.
"""

from __future__ import annotations

import os
import pathlib
import re
import sys
from urllib.parse import urlsplit

# The same restriction the fork applies to container names. Anything else is a
# typo or an injection attempt, and both should fail loudly here rather than
# produce a config that silently grants nothing and 401s three steps later.
CONTAINER = re.compile(r"\A[a-z0-9-]+\Z")

# `[azure-options."<host>".auth]` — the hosts already granted in the config.
TABLE_HEADER = re.compile(r'^\[azure-options\."([^"]+)"\.auth\]', re.MULTILINE)


def main() -> int:
    raw_channels = os.environ.get("AUTH_CHANNELS", "").split()
    if not raw_channels:
        return 0

    hosts: dict[str, set[str]] = {}
    for raw in raw_channels:
        url = urlsplit(raw)
        container = url.path.strip("/")
        if url.scheme != "az" or not url.netloc or not CONTAINER.fullmatch(container):
            sys.exit(f"not an az://<host>/<container> channel URL: {raw!r}")
        hosts.setdefault(url.netloc, set()).add(container)

    config_home = pathlib.Path(os.environ.get("XDG_CONFIG_HOME") or "~/.config")
    config = config_home.expanduser() / "pixi" / "config.toml"
    existing = config.read_text() if config.exists() else ""

    # Skip per host, not for the whole file: a second setup call in the same job
    # may name a host the first one didn't, and that host still needs its grant.
    # Appending a second table for a host that already has one would be a TOML
    # duplicate-key error, so an already-granted host is left exactly as it is —
    # including when this call names containers the existing table lacks. Merging
    # into an existing table would need a TOML round-trip (the stdlib reads TOML
    # but cannot write it), and no caller needs it: each call site lists every
    # channel its own packages read.
    granted = set(TABLE_HEADER.findall(existing))
    pending = {host: containers for host, containers in hosts.items() if host not in granted}
    if skipped := sorted(hosts.keys() & granted):
        print(f"already granted in {config}, left alone: {', '.join(skipped)}")
    if not pending:
        return 0

    # Built whole and written once: one table per host, with every container in
    # it, never two tables for one host.
    table = "".join(
        f'\n[azure-options."{host}".auth]\n'
        + "".join(f"{container} = true\n" for container in sorted(containers))
        for host, containers in sorted(pending.items())
    )
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(existing + table)
    total = sum(len(containers) for containers in pending.values())
    print(f"granted {total} container(s) across {len(pending)} host(s) in {config}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
