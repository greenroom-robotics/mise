# mise/.github/actions/setup

Provisions everything a pixi-based ROS package workflow needs to run mise CLI commands:

1. GitHub App token (so private deps clone)
2. Azure federated login (so the pixi fork can mint read SAS for the `az://` channels)
3. per-container credential grants for the channels named in `auth-channels` (see below)
4. the Greenroom pixi fork binary (native `az://` Azure Blob channel support; upstream pixi cannot read our `az://` channels)
5. a dedicated pixi tool manifest depending solely on `mise`, pinned to this action's version (read from this repo's root `pixi.toml`) and installed — so the calling repo does **not** need a root `pixi.toml` carrying `mise` as a dependency. The manifest path is exported as `$MISE_MANIFEST`; run mise with `pixi run --manifest-path "$MISE_MANIFEST" mise …`

Public action — call it directly from any workflow.

## Usage

```yaml
- uses: greenroom-robotics/mise/.github/actions/setup@v9
  with:
    gh-app-client-id: ${{ secrets.GH_APP_CLIENT_ID }}
    gh-app-private-key: ${{ secrets.GH_APP_PRIVATE_KEY }}
    azure-client-id: ${{ secrets.AZURE_CLIENT_ID }}
    azure-tenant-id: ${{ secrets.AZURE_TENANT_ID }}
    azure-subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}
    auth-channels: |
      az://<account>.blob.core.windows.net/general

# Do your work here, e.g. `pixi run --manifest-path "$MISE_MANIFEST" mise ci test`
```

## `auth-channels`

List every `az://` channel your manifests read, one URL per container. From pixi 0.75 a
request to a container with no grant is sent **unsigned**, and a private container answers
401 — the Azure login alone is not enough, because Azure never falls back to anonymous.

Grants cannot come from a repo's own `.pixi/config.toml`: pixi drops the whole
`azure-options` table from project-scoped config, so that cloning a repo can never make
your credentials sign requests to a host the repo names. That is why this is a workflow
input — the grant is declared by CI config you control, not by a manifest.

Do **not** list anonymous-read containers. Granting one attaches a bearer token, and an
identity with no role on it gets 403 where an unsigned request would have succeeded.

## Outputs

- `gh-token` — the GitHub App token, reused for downstream `gh` calls
