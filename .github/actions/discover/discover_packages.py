#!/usr/bin/env python3
"""Discover per-package pixi workspaces and emit a paths-filter map.

Reads env PACKAGE / PACKAGE_DIR / INCLUDE_WORKSPACES, writes `all`, `map` and
`dirs` outputs to the file named by GITHUB_OUTPUT. PACKAGE_DIR is one or more
directories, whitespace-separated (spaces and/or newlines both work —
`str.split()` treats either as a separator), so a caller can discover
packages fanned out across several directories (e.g. `libs\nprojects/foo`)
in one pass.

`all` is a compact JSON array of discovered dir-names, unique across every
given dir (the same name found under two dirs is an error — see
`discover_dirs`). `map` is a dorny/paths-filter YAML map where each entry's
filter is its own dir glob plus the dir globs of every dependency it
TRANSITIVELY path-depends on, so a change to a leaf retriggers every consumer
whose committed pixi.lock transitively pins it. `dirs` is a compact JSON
object mapping each name to the single package-dir it was discovered under,
for callers (like ci-test.yml's test matrix) that need to address a
manifest's dir without re-running discovery.

When INCLUDE_WORKSPACES=true, discovery also includes workspace-only
manifests (`[workspace]`, no `[package]`) that have a committed pixi.lock —
manifests with no publishable package but still pixi environments/tasks
`mise ci test` can install and run. A workspace's name is its dir basename
(same convention as a package), so it round-trips through
`<package-dir>/<name>/pixi.toml` the same way a package does.

# ponytail: package deps only follow single-level `path = "../x"` / `'../x'`
# sibling deps in a flat layout; deeper paths (`../../x`) and non-flat
# layouts are ignored. A package's sibling dep is resolved within the
# package's OWN dir only — one package-dir cannot path-dep into another.
# Workspace deps are resolved generically instead (see `dep_paths`), because
# a workspace manifest's path deps routinely point outside its own dir.
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


def load_manifest(manifest):
    try:
        with open(manifest, "rb") as f:
            return tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError):
        return None


def declares_package(manifest):
    """True when the manifest has a [package] table."""
    with open(manifest, "rb") as f:
        return "package" in tomllib.load(f)


def declares_workspace(manifest):
    """True when the manifest has a [workspace] table."""
    with open(manifest, "rb") as f:
        return "workspace" in tomllib.load(f)


def names_in_dir(package_dir):
    """Package ([package]-declaring) names directly under one package-dir."""
    return sorted(
        entry
        for entry in os.listdir(package_dir)
        if os.path.isfile(os.path.join(package_dir, entry, "pixi.toml"))
        and declares_package(os.path.join(package_dir, entry, "pixi.toml"))
    )


def workspace_names_in_dir(package_dir):
    """Workspace-only ([workspace], no [package]) names directly under one
    package-dir, restricted to ones with a committed pixi.lock — a workspace
    with no lock has nothing for `mise ci test` to install against
    reproducibly."""
    out = []
    for entry in sorted(os.listdir(package_dir)):
        sub = os.path.join(package_dir, entry)
        manifest = os.path.join(sub, "pixi.toml")
        if not os.path.isfile(manifest):
            continue
        if declares_package(manifest) or not declares_workspace(manifest):
            continue
        if not os.path.isfile(os.path.join(sub, "pixi.lock")):
            continue
        out.append(entry)
    return out


def discover_dirs(package_dirs, package, include_workspaces=False):
    """Map of name -> owning package-dir, across all package_dirs.

    A name found under more than one package-dir is ambiguous — error rather
    than silently pick one. When `package` is set, discovery is scoped to
    that single name (still resolved to whichever dir actually has it).
    """
    dirs = {}
    for package_dir in package_dirs:
        names = names_in_dir(package_dir)
        if include_workspaces:
            names = sorted(set(names) | set(workspace_names_in_dir(package_dir)))
        for name in names:
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


def dep_paths(manifest_dir):
    """Repo-relative directories `manifest_dir/pixi.toml` path-depends on
    directly, parsed generically from `[dependencies]` and every
    `[feature.*.dependencies]` table, resolved relative to `manifest_dir` —
    unlike a package's flat single-level sibling convention, these may point
    anywhere in the repo (e.g. `foo = { path = "../../src/foo" }`)."""
    data = load_manifest(os.path.join(manifest_dir, "pixi.toml"))
    if data is None:
        return set()
    tables = [data.get("dependencies", {})]
    tables.extend(
        feature.get("dependencies", {}) for feature in data.get("feature", {}).values()
    )
    paths = set()
    for table in tables:
        for spec in table.values():
            if isinstance(spec, dict) and "path" in spec:
                paths.add(os.path.normpath(os.path.join(manifest_dir, spec["path"])))
    return paths


def workspace_transitive_dep_paths(workspace_dir):
    """Forward path-dep closure of a workspace-only manifest's `path` deps,
    as repo-relative dirs — arbitrarily deep, and not restricted to
    `workspace_dir`'s own package-dir. See `dep_paths`."""
    start = os.path.normpath(workspace_dir)
    seen = set()
    stack = list(dep_paths(workspace_dir))
    while stack:
        dep_dir = stack.pop()
        if dep_dir in seen or dep_dir == start:
            continue
        seen.add(dep_dir)
        stack.extend(dep_paths(dep_dir))
    return seen


def build_map(dirs):
    lines = []
    for name in sorted(dirs):
        package_dir = dirs[name]
        entry_dir = os.path.join(package_dir, name)
        globs = [f"{package_dir}/{name}/**"]
        if declares_package(os.path.join(entry_dir, "pixi.toml")):
            for dep in sorted(transitive_deps(package_dir, name)):
                globs.append(f"{package_dir}/{dep}/**")
        else:
            for dep_dir in sorted(workspace_transitive_dep_paths(entry_dir)):
                globs.append(f"{dep_dir}/**")
        lines.append(f"{name}:")
        lines.extend(f"  - '{g}'" for g in globs)
    return "\n".join(lines)


def main():
    package = os.environ.get("PACKAGE", "")
    package_dirs = split_dirs(os.environ["PACKAGE_DIR"])
    include_workspaces = os.environ.get("INCLUDE_WORKSPACES", "false") == "true"
    if not package_dirs:
        print("::error::PACKAGE_DIR is empty")
        return 1
    dirs = discover_dirs(package_dirs, package, include_workspaces)
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

        os.mkdir(os.path.join(pkgdir, "devenv"))
        open(os.path.join(pkgdir, "devenv", "pixi.toml"), "w").write(
            '[workspace]\nname = "devenv"\n[tasks]\nbuild = "colcon build"\n'
        )
        assert discover_dirs([pkgdir], "") == {"a": pkgdir, "b": pkgdir, "c": pkgdir}, (
            "devenv must be skipped"
        )

        other = os.path.join(root, "other")
        os.mkdir(other)
        os.mkdir(os.path.join(other, "d"))
        open(os.path.join(other, "d", "pixi.toml"), "w").write('[package]\nname = "d"\n')
        multi = discover_dirs([pkgdir, other], "")
        assert multi == {"a": pkgdir, "b": pkgdir, "c": pkgdir, "d": other}
        assert split_dirs(f"{pkgdir}\n{other}") == [pkgdir, other]
        assert split_dirs(f"{pkgdir} {other}") == [pkgdir, other]

        dup = os.path.join(root, "dup")
        os.mkdir(dup)
        os.mkdir(os.path.join(dup, "a"))
        open(os.path.join(dup, "a", "pixi.toml"), "w").write('[package]\nname = "a"\n')
        try:
            discover_dirs([pkgdir, dup], "")
            raise AssertionError("expected SystemExit on name collision")
        except SystemExit:
            pass

        assert discover_dirs([pkgdir, other], "d") == {"d": other}

        # devenv has no pixi.lock: excluded even with include_workspaces.
        assert "devenv" not in discover_dirs([pkgdir], "", include_workspaces=True)

        # A locked workspace-only manifest is keyed by its dir basename, not
        # its [workspace] name.
        ws = os.path.join(pkgdir, "variant")
        os.mkdir(ws)
        open(os.path.join(ws, "pixi.toml"), "w").write(
            '[workspace]\nname = "totally_different_internal_name"\n'
            '[dependencies]\n'
            'b = { path = "../b" }\n'
        )
        open(os.path.join(ws, "pixi.lock"), "w").write("")
        with_ws = discover_dirs([pkgdir], "", include_workspaces=True)
        assert with_ws["variant"] == pkgdir
        assert "totally_different_internal_name" not in with_ws
        assert discover_dirs([pkgdir], "", include_workspaces=False) == {
            "a": pkgdir,
            "b": pkgdir,
            "c": pkgdir,
        }, "include_workspaces=False must not change existing behaviour"

        # Workspace path deps resolve generically and may point outside the
        # workspace's own package-dir entirely, at different `../` depths.
        deep = os.path.join(pkgdir, "deep")
        os.mkdir(deep)
        open(os.path.join(deep, "pixi.toml"), "w").write(
            '[workspace]\nname = "deep"\n'
            '[dependencies]\n'
            'mars_bringup = { path = "../../src/mars_bringup" }\n'
            '[feature.vessel.dependencies]\n'
            'gama_bringup = { path = "../../../src/gama_bringup" }\n'
        )
        open(os.path.join(deep, "pixi.lock"), "w").write("")
        two_up = os.path.normpath(os.path.join(deep, "../../src/mars_bringup"))
        three_up = os.path.normpath(os.path.join(deep, "../../../src/gama_bringup"))
        got = workspace_transitive_dep_paths(deep)
        assert two_up in got
        assert three_up in got

        m = build_map({"deep": pkgdir})
        assert f"{pkgdir}/deep/**" in m
        assert f"{two_up}/**" in m
        assert f"{three_up}/**" in m

        m2 = build_map({"a": pkgdir})
        assert m2 == f"a:\n  - '{pkgdir}/a/**'\n  - '{pkgdir}/b/**'\n  - '{pkgdir}/c/**'"
    print("selftest ok")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "selftest":
        selftest()
    else:
        sys.exit(main())
