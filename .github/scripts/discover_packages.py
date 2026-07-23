#!/usr/bin/env python3
"""Discover per-package pixi workspaces and emit a paths-filter map.

Reads env PACKAGE / PACKAGE_DIR, writes `all` and `map` outputs to the file
named by GITHUB_OUTPUT. `all` is a compact JSON array of package dir-names;
`map` is a dorny/paths-filter YAML map where each package's filter is its own
dir glob plus the dir globs of every sibling it TRANSITIVELY path-depends on,
so a change to a leaf retriggers every consumer whose committed pixi.lock
transitively pins it.

# ponytail: only single-level `path = "../x"` / `'../x'` sibling deps in a flat
# layout are followed. Deeper paths (`../../x`) and non-flat layouts are ignored.
"""

import json
import os
import re
import sys

# Matches `path = "../NAME"` and `path = '../NAME'` where NAME has no slash.
PATH_DEP_RE = re.compile(r"""path\s*=\s*["']\.\./([^/"']+)["']""")


def discover_names(package_dir, package):
    if package:
        return [package]
    names = [
        entry
        for entry in os.listdir(package_dir)
        if os.path.isfile(os.path.join(package_dir, entry, "pixi.toml"))
    ]
    return sorted(names)


def direct_deps(package_dir, name):
    """Flat single-level sibling names this package path-depends on directly."""
    manifest = os.path.join(package_dir, name, "pixi.toml")
    try:
        text = open(manifest, encoding="utf-8").read()
    except OSError:
        return set()
    return set(PATH_DEP_RE.findall(text))


def transitive_deps(package_dir, name):
    """Forward path-dep closure of `name` (excluding `name` itself)."""
    seen = set()
    stack = list(direct_deps(package_dir, name))
    while stack:
        dep = stack.pop()
        if dep in seen or dep == name:
            continue
        seen.add(dep)
        stack.extend(direct_deps(package_dir, dep))
    return seen


def build_map(package_dir, names):
    lines = []
    for name in names:
        globs = [f"{package_dir}/{name}/**"]
        for dep in sorted(transitive_deps(package_dir, name)):
            globs.append(f"{package_dir}/{dep}/**")
        lines.append(f"{name}:")
        lines.extend(f"  - '{g}'" for g in globs)
    return "\n".join(lines)


def main():
    package = os.environ.get("PACKAGE", "")
    package_dir = os.environ["PACKAGE_DIR"]
    names = discover_names(package_dir, package)
    if not names:
        print(f"::error::no packages with pixi.toml found under {package_dir}")
        return 1
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
        out.write(f"all={json.dumps(names, separators=(',', ':'))}\n")
        out.write("map<<__EOF__\n")
        out.write(build_map(package_dir, names) + "\n")
        out.write("__EOF__\n")
    return 0


def selftest():
    import tempfile

    with tempfile.TemporaryDirectory() as root:
        for pkg, dep in [("a", "b"), ("b", "c"), ("c", None)]:
            os.mkdir(os.path.join(root, pkg))
            body = '[package]\nname = "%s"\n' % pkg
            if dep:
                body += '[dependencies]\n%s = { path = "../%s" }\n' % (dep, dep)
            open(os.path.join(root, pkg, "pixi.toml"), "w").write(body)
        assert transitive_deps(root, "a") == {"b", "c"}, "A must reach B and C"
        assert transitive_deps(root, "b") == {"c"}
        assert transitive_deps(root, "c") == set()
    print("selftest ok")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "selftest":
        selftest()
    else:
        sys.exit(main())
