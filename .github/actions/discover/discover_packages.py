#!/usr/bin/env python3
"""Discover per-package pixi workspaces and emit a paths-filter map.

Reads env PACKAGE / PACKAGE_DIR, writes `all`, `map` and `dirs` outputs to the
file named by GITHUB_OUTPUT. PACKAGE_DIR is one or more directories,
whitespace-separated (spaces and/or newlines both work — `str.split()` treats
either as a separator), so a caller can discover packages fanned out across
several directories (e.g. `libs\nprojects/gama_vessel_variants`) in one pass.

`all` is a compact JSON array of package dir-names, unique across every given
dir (the same name found under two dirs is an error — see `discover_dirs`).
`map` is a dorny/paths-filter YAML map where each package's filter is its own
dir glob plus the dir globs of every sibling it TRANSITIVELY path-depends on,
so a change to a leaf retriggers every consumer whose committed pixi.lock
transitively pins it. `dirs` is a compact JSON object mapping each package
name to the single package-dir it was discovered under, for callers (like
ci-test.yml's test matrix) that need to address a package's manifest without
re-running discovery.

# ponytail: only single-level `path = "../x"` / `'../x'` sibling deps in a flat
# layout are followed. Deeper paths (`../../x`) and non-flat layouts are
# ignored. A sibling dep is resolved within the package's OWN dir only — a
# package under one package-dir cannot path-dep on a package under another.
"""

import json
import os
import re
import sys
import tomllib

# Matches `path = "../NAME"` and `path = '../NAME'` where NAME has no slash.
PATH_DEP_RE = re.compile(r"""path\s*=\s*["']\.\./([^/"']+)["']""")


def split_dirs(package_dir):
    """One or more package-dirs from a whitespace-separated string."""
    return package_dir.split()


def declares_package(manifest):
    """True when the manifest has a [package] table.

    Workspace-only manifests are dev environments for packages this repo does
    not publish (e.g. one built from a hand-authored recipe elsewhere); they
    have no tests to fan out to a matrix leg. Mirrors `declares_package` in
    src/commands/ci/packages.rs — keep the two in step.
    """
    with open(manifest, "rb") as f:
        return "package" in tomllib.load(f)


def names_in_dir(package_dir):
    """Package names directly under one package-dir."""
    return sorted(
        entry
        for entry in os.listdir(package_dir)
        if os.path.isfile(os.path.join(package_dir, entry, "pixi.toml"))
        and declares_package(os.path.join(package_dir, entry, "pixi.toml"))
    )


def discover_dirs(package_dirs, package):
    """Map of package name -> owning package-dir, across all package_dirs.

    A name found under more than one package-dir is ambiguous — error rather
    than silently pick one. When `package` is set, discovery is scoped to
    that single name (still resolved to whichever dir actually has it).
    """
    dirs = {}
    for package_dir in package_dirs:
        for name in names_in_dir(package_dir):
            if package and name != package:
                continue
            if name in dirs:
                raise SystemExit(
                    f"::error::package {name!r} found under both "
                    f"{dirs[name]!r} and {package_dir!r} — package names "
                    "must be unique across package-dir"
                )
            dirs[name] = package_dir
    if package and package not in dirs:
        raise SystemExit(f"::error::package {package!r} not found under any of {package_dirs}")
    return dirs


def direct_deps(package_dir, name):
    """Flat single-level sibling names this package path-depends on directly."""
    manifest = os.path.join(package_dir, name, "pixi.toml")
    try:
        text = open(manifest, encoding="utf-8").read()
    except OSError:
        return set()
    return set(PATH_DEP_RE.findall(text))


def transitive_deps(package_dir, name):
    """Forward path-dep closure of `name` (excluding `name` itself), scoped
    to siblings within `package_dir` — the same dir `name` was found in."""
    seen = set()
    stack = list(direct_deps(package_dir, name))
    while stack:
        dep = stack.pop()
        if dep in seen or dep == name:
            continue
        seen.add(dep)
        stack.extend(direct_deps(package_dir, dep))
    return seen


def build_map(dirs):
    lines = []
    for name in sorted(dirs):
        package_dir = dirs[name]
        globs = [f"{package_dir}/{name}/**"]
        for dep in sorted(transitive_deps(package_dir, name)):
            globs.append(f"{package_dir}/{dep}/**")
        lines.append(f"{name}:")
        lines.extend(f"  - '{g}'" for g in globs)
    return "\n".join(lines)


def main():
    package = os.environ.get("PACKAGE", "")
    package_dirs = split_dirs(os.environ["PACKAGE_DIR"])
    if not package_dirs:
        print("::error::PACKAGE_DIR is empty")
        return 1
    dirs = discover_dirs(package_dirs, package)
    if not dirs:
        print(f"::error::no packages with pixi.toml found under {package_dirs}")
        return 1
    names = sorted(dirs)
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
        out.write(f"all={json.dumps(names, separators=(',', ':'))}\n")
        out.write(f"dirs={json.dumps(dirs, separators=(',', ':'))}\n")
        out.write("map<<__EOF__\n")
        out.write(build_map(dirs) + "\n")
        out.write("__EOF__\n")
    return 0


def selftest():
    import tempfile

    with tempfile.TemporaryDirectory() as root:
        pkgdir = os.path.join(root, "packages")
        os.mkdir(pkgdir)
        for pkg, dep in [("a", "b"), ("b", "c"), ("c", None)]:
            os.mkdir(os.path.join(pkgdir, pkg))
            body = '[package]\nname = "%s"\n' % pkg
            if dep:
                body += '[dependencies]\n%s = { path = "../%s" }\n' % (dep, dep)
            open(os.path.join(pkgdir, pkg, "pixi.toml"), "w").write(body)
        assert transitive_deps(pkgdir, "a") == {"b", "c"}, "A must reach B and C"
        assert transitive_deps(pkgdir, "b") == {"c"}
        assert transitive_deps(pkgdir, "c") == set()
        assert discover_dirs([pkgdir], "") == {"a": pkgdir, "b": pkgdir, "c": pkgdir}

        # A workspace-only manifest is a dev env, not a matrix leg.
        os.mkdir(os.path.join(pkgdir, "devenv"))
        open(os.path.join(pkgdir, "devenv", "pixi.toml"), "w").write(
            '[workspace]\nname = "devenv"\n[tasks]\nbuild = "colcon build"\n'
        )
        assert discover_dirs([pkgdir], "") == {"a": pkgdir, "b": pkgdir, "c": pkgdir}, (
            "devenv must be skipped"
        )

        # Multi-dir: union across two package-dirs, names stay attributed to
        # their own dir, and deps stay scoped within a dir (no cross-dir dep).
        other = os.path.join(root, "other")
        os.mkdir(other)
        os.mkdir(os.path.join(other, "d"))
        open(os.path.join(other, "d", "pixi.toml"), "w").write('[package]\nname = "d"\n')
        multi = discover_dirs([pkgdir, other], "")
        assert multi == {"a": pkgdir, "b": pkgdir, "c": pkgdir, "d": other}
        assert split_dirs(f"{pkgdir}\n{other}") == [pkgdir, other]
        assert split_dirs(f"{pkgdir} {other}") == [pkgdir, other]

        # Name collision across dirs is an error, not a silent pick.
        dup = os.path.join(root, "dup")
        os.mkdir(dup)
        os.mkdir(os.path.join(dup, "a"))
        open(os.path.join(dup, "a", "pixi.toml"), "w").write('[package]\nname = "a"\n')
        try:
            discover_dirs([pkgdir, dup], "")
            raise AssertionError("expected SystemExit on name collision")
        except SystemExit:
            pass

        # `package` filter scopes discovery to one name, resolved to its dir.
        assert discover_dirs([pkgdir, other], "d") == {"d": other}
    print("selftest ok")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "selftest":
        selftest()
    else:
        sys.exit(main())
