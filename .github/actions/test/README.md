# mise/.github/actions/test

One-liner CI step for pixi-native ROS package repos. Sets up the environment (GitHub App token, Azure login, CCP, pixi), runs `mise ci test`, and tears down the proxy.

`mise ci test` runs one or more `<env>:<task>` jobs per package (failing at the end, not fail-fast) and collects each package's JUnit XML from the standard colcon `build/` location into `report-dir`, namespaced by env so jobs that share a `build/` dir don't overwrite each other's reports. By default it runs a single `tests:test` job; pass the `jobs` input to fan out across environments (e.g. a Boost-Asio build variant and a lint env alongside the standard tests). The action then uploads `report-dir` as the `pixi-test-reports` artifact and publishes a rendered test-report check named `Test Report (pixi)` — distinct from the legacy deb path's `Test Report` check so the two coexist on the same commit.

If the test task emits no JUnit XML (e.g. `cargo nextest`), report collection and the publish step are skipped silently — the action's success still reflects whether the tests passed.

## Usage

```yaml
jobs:
  test:
    runs-on: 2vcpu-ubuntu-2404
    steps:
      - uses: actions/checkout@v6
      - uses: greenroom-robotics/mise/.github/actions/test@v4
        with:
          gh-app-client-id: ${{ secrets.GH_APP_CLIENT_ID }}
          gh-app-private-key: ${{ secrets.GH_APP_PRIVATE_KEY }}
          azure-client-id: ${{ secrets.AZURE_CLIENT_ID }}
          azure-tenant-id: ${{ secrets.AZURE_TENANT_ID }}
          azure-subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}
```

That's the whole workflow. Add `package` or `package-dir` inputs to filter or rename defaults, `report-dir` to change where JUnit XML is collected, or `check-name` to rename the rendered report check.

### Running multiple environments

To fan out across environments — for example a standard build, a Boost-Asio build variant, and a lint env — pass `jobs` as newline-separated `<env>:<task>` pairs:

```yaml
      - uses: greenroom-robotics/mise/.github/actions/test@v4
        with:
          jobs: |
            tests:test
            tests-boost:test
            lint:lint
          # ...secrets as above
```

Each line runs as a separate `pixi run -e <env> <task>`, and reports are collected under `<report-dir>/<package>/<env>/` so variants that share a `build/` dir don't clobber each other.

## Contract on the consumer

The consumer's workspace `pixi.toml` must define a `tests` environment with a `test` task (and any extra environments referenced via `jobs`). Example:

```toml
[feature.tests.dependencies]
pytest = "*"

[feature.tests.tasks]
test = "pytest packages/"

[environments]
tests = ["tests"]
```
