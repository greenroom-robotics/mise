# `discover` action

Discovers per-package pixi workspaces under `package-dir` and emits a
`dorny/paths-filter` map for `ci-test.yml`'s matrix. Runs in the CALLER's
checkout, so it ships its own script (`discover_packages.py`, resolved via
`github.action_path`) rather than relying on a repo-local `.github/scripts/`
path — reusable-workflow steps execute in the caller's checkout, not this
repo's, and a path into this repo would 404 for every external caller.

A subdir whose `pixi.toml` has no `[package]` table is skipped: that's a
workspace-only manifest — a dev/test environment for something this repo
doesn't publish (e.g. a package built from a hand-authored recipe in
ros-recipes) — so it has no matrix leg and nothing to release. `mise ci`'s own
discovery skips it the same way.

`package-dir` accepts one or more directories, whitespace-separated (space or
newline both work), e.g. `libs` or `libs\nprojects/gama_vessel_variants`.
Packages are unioned across every dir given; a name found under more than one
dir is an error. A path-dep (`path = "../sibling"`) is only followed within
the dir the package itself was found in — it never crosses dirs.

`all` is a compact JSON array of package dir-names. `map` is a
`dorny/paths-filter` YAML map where each package's filter is its own dir glob
plus the dir globs of every sibling it TRANSITIVELY path-depends on (via
`path = "../sibling"` in its `pixi.toml`), so a change to a leaf retriggers
every consumer whose committed `pixi.lock` transitively pins it. `dirs` is a
compact JSON object mapping each package name to the single package-dir it
was found under — useful for a caller (like `ci-test.yml`'s test matrix) that
needs to address one package's manifest without re-running discovery.

Requires a full-history checkout beforehand (`fetch-depth: 0`) when the
caller's workflow diffs against a base ref (e.g. `paths-filter` on a PR).

## Usage

```yaml
jobs:
  discover:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - id: all
        uses: greenroom-robotics/mise/.github/actions/discover@v8
        with:
          package: ${{ inputs.package }}       # empty = discover every package
          package-dir: ${{ inputs.package-dir }} # default: packages
      - uses: dorny/paths-filter@v4
        with:
          filters: ${{ steps.all.outputs.map }}
```

See `action.yml` for the full input/output list.
