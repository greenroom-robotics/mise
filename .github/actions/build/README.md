# mise/.github/actions/build

One-liner CI step for pixi-native ROS package repos. Sets up the environment, runs `pixi run mise ci build`, stages outputs at `$RUNNER_TEMP/conda-bld`, and tears down the proxy.

## Usage

```yaml
jobs:
  build:
    runs-on: 2vcpu-ubuntu-2404
    steps:
      - uses: actions/checkout@v6
      - uses: greenroom-robotics/mise/.github/actions/build@v2
        with:
          gh-app-id: ${{ secrets.GH_APP_ID }}
          gh-app-private-key: ${{ secrets.GH_APP_PRIVATE_KEY }}
          azure-client-id: ${{ vars.AZURE_READER_CLIENT_ID }}
          azure-tenant-id: ${{ vars.AZURE_TENANT_ID }}
          azure-subscription-id: ${{ vars.AZURE_SUBSCRIPTION_ID }}
          target-platform: linux-64
```

Built `.conda` artifacts land at `$RUNNER_TEMP/conda-bld/<subdir>/`. Upload via `actions/upload-artifact@v7` if you need them later in the same workflow.
