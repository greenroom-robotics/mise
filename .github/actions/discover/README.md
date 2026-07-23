# `discover` action

Discovers per-package pixi workspaces under `package-dir` and emits a
`dorny/paths-filter` map for `ci-test.yml`'s matrix. Runs in the CALLER's
checkout, so it ships its own script (`discover_packages.py`, resolved via
`github.action_path`) rather than relying on a repo-local `.github/scripts/`
path — reusable-workflow steps execute in the caller's checkout, not this
repo's, and a path into this repo would 404 for every external caller.

`all` is a compact JSON array of package dir-names. `map` is a
`dorny/paths-filter` YAML map where each package's filter is its own dir glob
plus the dir globs of every sibling it TRANSITIVELY path-depends on (via
`path = "../sibling"` in its `pixi.toml`), so a change to a leaf retriggers
every consumer whose committed `pixi.lock` transitively pins it.

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
        uses: greenroom-robotics/mise/.github/actions/discover@v6
        with:
          package: ${{ inputs.package }}       # empty = discover every package
          package-dir: ${{ inputs.package-dir }} # default: packages
      - uses: dorny/paths-filter@v4
        with:
          filters: ${{ steps.all.outputs.map }}
```

See `action.yml` for the full input/output list.
