# `recipes-pr` action

Publishes conda recipe PRs for packages the **deb** release just tagged. Use it
as the pixi half of a unified deb+pixi release: the deb side
(`ros_semantic_release_action`) owns tagging and cuts `<pkg>@<version>` tags;
this action discovers the tags created *this run* (those not already ancestors
of `dispatch-sha`) and runs `mise ci recipes-pr` per package to open/update a
PR on the recipes repo.

It bundles `mise/setup`, fetches tags, runs the publish loop, and tears down the
conda-channel-proxy. No semantic-release, no tagging.

## Usage

```yaml
jobs:
  pixi:
    needs: deb            # deb job creates the tags
    runs-on: 2vcpu-ubuntu-2404
    permissions:
      contents: read
      id-token: write
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0   # full history so the ancestor check works
      - uses: greenroom-robotics/mise/.github/actions/recipes-pr@v4
        with:
          dispatch-sha: ${{ github.sha }}
          package: ${{ inputs.package }}    # empty = all released this run
          ros-distro: kilted
          gh-app-client-id: ${{ secrets.GH_APP_CLIENT_ID }}
          gh-app-private-key: ${{ secrets.GH_APP_PRIVATE_KEY }}
          azure-client-id: ${{ secrets.AZURE_CLIENT_ID }}
          azure-tenant-id: ${{ secrets.AZURE_TENANT_ID }}
          azure-subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}
```

See `action.yml` for the full input list (`package-dir`, `recipes-repo`,
`proxy-version` all default sensibly).
