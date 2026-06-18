# mise/.github/actions/release

One-liner CI step for pixi-native ROS package repos to run their release process. Sets up the environment (gh-app-token, Azure login, CCP, pixi, Node, semantic-release deps), runs `mise ci release`, and tears down the proxy.

## Usage

```yaml
name: Release
on:
  workflow_dispatch:
    inputs:
      package:
        type: string
        default: ""

jobs:
  release:
    runs-on: 2vcpu-ubuntu-2404
    permissions:
      contents: write
      pull-requests: write
      id-token: write
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: greenroom-robotics/mise/.github/actions/release@v4
        with:
          package: ${{ inputs.package }}
          gh-app-id: ${{ secrets.GH_APP_ID }}
          gh-app-private-key: ${{ secrets.GH_APP_PRIVATE_KEY }}
          azure-client-id: ${{ secrets.AZURE_CLIENT_ID }}
          azure-tenant-id: ${{ secrets.AZURE_TENANT_ID }}
          azure-subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}
```

`fetch-depth: 0` is required — semantic-release reads the full commit history to compute the next version.

## What happens

1. The setup action provisions a GitHub App token, Azure credentials, the conda-channel-proxy, and a pixi environment.
2. Node 20 is installed alongside `yarn` (via `actions/setup-node`).
3. `release-tooling/package.json` and `release-tooling/yarn.lock` are copied into the workspace root.
4. `yarn install --frozen-lockfile` brings in semantic-release + plugins.
5. `mise ci release ...` writes a `.releaserc` for each package, then runs semantic-release (or multi-semantic-release for the multi-package case).
6. semantic-release's `@semantic-release/exec` plugin calls back into `mise ci recipes-pr` to open/update the recipes-repo PR.
7. The conda-channel-proxy teardown action runs unconditionally.

## Recipes-repo contract

The recipes repo must accept PRs from greenroom-bot. `mise ci recipes-pr` clones the repo using `API_TOKEN_GITHUB`, creates a version-independent branch `release/<source-repo>`, upserts the package's entry in `rosdistro_additional_recipes.yaml`, and opens (or force-pushes onto an existing) PR. The branch name omits the version on purpose: every release of a source repo lands on the same rolling PR, so a newer release always supersedes an older one instead of leaving stale per-version PRs open.

## Caller responsibilities

- Set `permissions: contents: write` (semantic-release commits CHANGELOG.md back to the source repo).
- Set `permissions: pull-requests: write` (so `gh pr create` works against the recipes repo via the App token).
- The GitHub App used here needs `Contents: write` and `Pull requests: write` on both the source repo and the recipes repo.
