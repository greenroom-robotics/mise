# mise/.github/actions/lint

One-liner CI step that runs the calling repo's `.pre-commit-config.yaml` over all files. Sets up the environment (GitHub App token, Azure login, pixi fork), then runs `pre-commit` through `pixi exec` with the `lint-tools-meta` conda package, which provides `python`, `pre-commit` and `clang-format`.

The calling repo needs no pixi workspace for this: `pixi exec` builds a throwaway environment, so the repo's root manifest (and its build backend) is never solved. Each repo keeps its own `.pre-commit-config.yaml`; hooks that call `clang-format` should use `language: system` so they pick up the one on `PATH`.

The `~/.cache/pre-commit` hook environments are cached per runner OS/arch and the hash of `.pre-commit-config.yaml`.

## Usage

```yaml
jobs:
  lint:
    runs-on: 2vcpu-ubuntu-2404
    permissions:
      contents: read
      id-token: write
    steps:
      - uses: actions/checkout@v6
      - uses: greenroom-robotics/mise/.github/actions/lint@v8
        with:
          gh-app-client-id: ${{ secrets.GH_APP_CLIENT_ID }}
          gh-app-private-key: ${{ secrets.GH_APP_PRIVATE_KEY }}
          azure-client-id: ${{ secrets.AZURE_CLIENT_ID }}
          azure-tenant-id: ${{ secrets.AZURE_TENANT_ID }}
          azure-subscription-id: ${{ secrets.AZURE_SUBSCRIPTION_ID }}
```

Add `skip-hooks` (comma-separated hook ids, e.g. `skip-hooks: codespell,clang-format`) to skip hooks in CI. For the whole job wrapped as a reusable workflow, see `.github/workflows/ci-lint.yml`.
