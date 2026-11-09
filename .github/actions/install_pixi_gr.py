#!/usr/bin/env python3
"""Install the Greenroom pixi fork from an authenticated GitHub release.

Usage: install_pixi_gr.py <version>   (token read from GH_TOKEN)
"""

import gzip
import json
import os
import platform
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

API = "https://api.github.com/repos/greenroom-robotics/pixi/releases"
ARCHES = {
    "x86_64": "linux-64",
    "amd64": "linux-64",
    "aarch64": "linux-aarch64",
    "arm64": "linux-aarch64",
}
ATTEMPTS = 5
TIMEOUT_SECONDS = 60


def arch_for(machine: str) -> str:
    try:
        return ARCHES[machine.lower()]
    except KeyError:
        sys.exit(f"unsupported architecture: {machine}")


def is_transient(error: Exception) -> bool:
    if isinstance(error, urllib.error.HTTPError):
        return error.code == 429 or error.code >= 500
    return isinstance(error, (urllib.error.URLError, TimeoutError, ConnectionError))


def build_request(url: str, token: str, accept: str) -> urllib.request.Request:
    request = urllib.request.Request(url, headers={"Accept": accept})
    # Unredirected: the asset endpoint redirects to a pre-signed URL on another host.
    request.add_unredirected_header("Authorization", f"Bearer {token}")
    return request


def fetch(url: str, token: str, accept: str) -> bytes:
    for attempt in range(1, ATTEMPTS + 1):
        try:
            with urllib.request.urlopen(
                build_request(url, token, accept), timeout=TIMEOUT_SECONDS
            ) as response:
                return response.read()
        except Exception as error:
            if not is_transient(error) or attempt == ATTEMPTS:
                raise
            delay = 2**attempt
            print(f"{url}: {error}; retry {attempt}/{ATTEMPTS - 1} in {delay}s", file=sys.stderr)
            time.sleep(delay)
    raise AssertionError("unreachable")


def asset_id(version: str, arch: str, token: str) -> int:
    body = fetch(f"{API}/tags/pixi-gr@{version}", token, "application/vnd.github+json")
    name = f"pixi-{arch}.gz"
    for asset in json.loads(body)["assets"]:
        if asset["name"] == name:
            return int(asset["id"])
    sys.exit(f"release pixi-gr@{version} has no asset {name}")


def write_executable(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".pixi-")
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(content)
        os.chmod(tmp, 0o755)
        os.replace(tmp, path)
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise


def export_path(github_path: Path) -> None:
    home = Path.home()
    with github_path.open("a") as handle:
        handle.write(f"{home / '.local' / 'bin'}\n{home / '.pixi' / 'bin'}\n")


def main(argv: list[str]) -> None:
    if len(argv) != 2:
        sys.exit("usage: install_pixi_gr.py <version>")
    version = argv[1]
    token = os.environ.get("GH_TOKEN")
    if not token:
        sys.exit("GH_TOKEN is not set")

    arch = arch_for(platform.machine())
    blob = fetch(
        f"{API}/assets/{asset_id(version, arch, token)}", token, "application/octet-stream"
    )
    binary = gzip.decompress(blob)

    pixi_home = Path(os.environ.get("PIXI_HOME") or Path.home() / ".pixi")
    write_executable(pixi_home / "bin" / "pixi", binary)

    github_path = os.environ.get("GITHUB_PATH")
    if github_path:
        export_path(Path(github_path))


if __name__ == "__main__":
    main(sys.argv)
