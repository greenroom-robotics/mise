# mise/.github/actions/test

One-liner CI step for pixi-native ROS package repos. Sets up the environment (GitHub App token, Azure login, CCP, pixi), runs `pixi run mise ci test`, and tears down the proxy.

`mise ci test` runs every package's `tests` task (failing at the end, not fail-fast) and collects each package's JUnit XML from the standard colcon `build/` location into `report-dir`. The action then uploads that directory as the `pixi-test-reports` artifact and publishes a rendered test-report check named `Test Report (pixi)` — distinct from the legacy deb path's `Test Report` check so the two coexist on the same commit.

## Usage

```yaml
jobs:
  test:
    runs-on: 2vcpu-ubuntu-2404
    steps:
      - uses: actions/checkout@v6
      - uses: greenroom-robotics/mise/.github/actions/test@v4
        with:
          gh-app-id: ${{ secrets.GH_APP_ID }}
          gh-app-private-key: ${{ secrets.GH_APP_PRIVATE_KEY }}
          azure-client-id: ${{ secrets.AZURE_CLIENT_ID }}
          azure-tenant-id: ${{ secrets.AZURE_TENANT_ID }}
          azure-subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}
```

That's the whole workflow. Add `package` or `package-dir` inputs to filter or rename defaults, `report-dir` to change where JUnit XML is collected, or `check-name` to rename the rendered report check.

## Contract on the consumer

The consumer's workspace `pixi.toml` must define a `tests` environment with a `test` task. Example:

```toml
[feature.tests.dependencies]
pytest = "*"

[feature.tests.tasks]
test = "pytest packages/"

[environments]
tests = ["tests"]
```
