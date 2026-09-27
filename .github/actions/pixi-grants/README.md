# mise/.github/actions/pixi-grants

Merges the Azure channel grants from ros-recipes (`ci/azure-options.toml`) into `~/.pixi/config.toml`. Only the `azure-options` key is replaced. Other keys, comments and formatting stay as they are. The container names are masked in the log.

The action is idempotent. You can use it without pixi installed, for example to make a config that you then copy into a container build.

## Usage

```yaml
- uses: greenroom-robotics/mise/.github/actions/pixi-grants@v8
  with:
    gh-token: ${{ steps.setup.outputs.gh-token }}
```
