# mise

Build and release automation for Greenroom's conda/pixi ROS packaging: the `mise` CLI plus the composite GitHub Actions that wrap it.

## CLI

```
mise matrix        # compute the CI build matrix (vinca, pixi-native, DeepStream)
mise build-recipes # run the builds (vinca / pixi-native / DeepStream container)
mise ci            # test, build, and release pixi-native ROS package repos
mise snapshot      # refresh rosdistro_snapshot.yaml and the vinca-cache repodata
```

`mise ci` also carries the semantic-release hook callbacks (`prepare`, `publish`, `recipes-pr`, …). They exist for semantic-release to invoke, not for you to run by hand.

## Actions

Each is a composite action under `.github/actions/`, versioned with the repo (`@v8`):

| action | purpose |
| --- | --- |
| `setup` | GitHub App token, Azure credentials, the Greenroom pixi fork, and the pinned `mise` CLI. Use this to build custom workflows. |
| `test` | `mise ci test` |
| `build` | `mise ci build`, emitting `.conda` artifacts under `$RUNNER_TEMP/conda-bld` |
| `release` | `mise ci release` (semantic-release) |
| `recipes-pr` | open/update the conda recipe PR for the `<pkg>@<version>` tags a deb release just cut |
| `discover` | find per-package pixi workspaces and emit a `paths-filter` map keyed by package |

## Development

```sh
cargo test
cargo run -- --help
```

See `~/Repositories/platform/docs/pixi.md` for the wider pixi workflow this fits into. Releases are cut by semantic-release; the version in `Cargo.toml` is synced by the `ci prepare` hook.
