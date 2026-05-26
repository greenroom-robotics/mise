# mise/.github/actions/setup

Provisions everything a pixi-based ROS package workflow needs to run mise CLI commands:

1. GitHub App token (so private deps clone)
2. Azure federated login (so the conda-channel-proxy can reach the storage account)
3. `conda-channel-proxy` running on `http://localhost:8000`, serving the `general` and `overrides` channels under `/general` and `/overrides`
4. pixi installed with the workspace lockfile applied

Public action — call it directly from any workflow.

## Usage

```yaml
- uses: greenroom-robotics/mise/.github/actions/setup@v2
  with:
    gh-app-id: ${{ secrets.GH_APP_ID }}
    gh-app-private-key: ${{ secrets.GH_APP_PRIVATE_KEY }}
    azure-client-id: ${{ vars.AZURE_READER_CLIENT_ID }}
    azure-tenant-id: ${{ vars.AZURE_TENANT_ID }}
    azure-subscription-id: ${{ vars.AZURE_SUBSCRIPTION_ID }}

# Do your work here, e.g. `pixi run mise ci test`

- if: always()
  uses: greenroom-robotics/conda-channel-proxy/.github/actions/teardown@v2
```

**Teardown is your responsibility.** This action does not stop the proxy. Pair it with the CCP `teardown` action under `if: always()` so the proxy is stopped and the log is uploaded on both success and failure.

## Outputs

- `proxy-url` — `http://localhost:<port>` base URL of the running proxy. Channels live under `/general` and `/overrides`; prefer the per-channel outputs below for downstream channel references.
- `proxy-general-url` — `http://localhost:<port>/general`, the main private channel.
- `proxy-overrides-url` — `http://localhost:<port>/overrides`, the overrides channel (used to shadow `general` with patched builds).
- `gh-token` — the GitHub App token, reused for downstream `gh` calls
