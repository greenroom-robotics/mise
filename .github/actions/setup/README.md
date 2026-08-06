# mise/.github/actions/setup

Provisions everything a pixi-based ROS package workflow needs to run mise CLI commands:

1. GitHub App token (so private deps clone)
2. Azure federated login (so the pixi fork can mint read SAS for the `az://` channels)
3. the Greenroom pixi fork binary (native `az://` Azure Blob channel support; upstream pixi cannot read our `az://` channels)
4. a dedicated pixi tool manifest depending solely on `mise`, pinned to this action's version (read from this repo's root `pixi.toml`) and installed — so the calling repo does **not** need a root `pixi.toml` carrying `mise` as a dependency. The manifest path is exported as `$MISE_MANIFEST`; run mise with `pixi run --manifest-path "$MISE_MANIFEST" mise …`

Public action — call it directly from any workflow.

## Usage

```yaml
- uses: greenroom-robotics/mise/.github/actions/setup@v8
  with:
    gh-app-client-id: ${{ secrets.GH_APP_CLIENT_ID }}
    gh-app-private-key: ${{ secrets.GH_APP_PRIVATE_KEY }}
    azure-client-id: ${{ secrets.AZURE_CLIENT_ID }}
    azure-tenant-id: ${{ secrets.AZURE_TENANT_ID }}
    azure-subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}

# Do your work here, e.g. `pixi run --manifest-path "$MISE_MANIFEST" mise ci test`
```

## Outputs

- `gh-token` — the GitHub App token, reused for downstream `gh` calls
