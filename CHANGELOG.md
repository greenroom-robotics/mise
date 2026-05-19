## 1.0.0 (2026-05-19)

### Features

* add --repo-root to bump and snapshot subcommands ([35a88b9](https://github.com/greenroom-robotics/mise/commit/35a88b99bc896550fa0d17942529e671a44821f6))
* add --runner-size flag to build pixi ([f0adfea](https://github.com/greenroom-robotics/mise/commit/f0adfeae82b000d08a73d3f9250745b040cffd4b))
* add Pipeline and MatrixEntry types for build matrix ([f2ae7a1](https://github.com/greenroom-robotics/mise/commit/f2ae7a1cd885840314d9ef7227a38e446f332a96))
* add recipe-dir filter and variants-pin helpers for build vinca ([3adffc8](https://github.com/greenroom-robotics/mise/commit/3adffc84464cbc4faf33195b69ab216910e99b7d))
* add Repo discovery and typed YAML loaders ([aca9a80](https://github.com/greenroom-robotics/mise/commit/aca9a80cc62afd47ce46fc60548a74afabc30000))
* add subprocess helpers with stderr-on-error context ([2330c7d](https://github.com/greenroom-robotics/mise/commit/2330c7d402cb66c9eaa855194550c2bb9255c8a6))
* add typed GitHub Actions event parsing and outputs helper ([88a7fae](https://github.com/greenroom-robotics/mise/commit/88a7fae523d9de51878253743ea309d325a645b2))
* add upstream pixi.toml parser for build pixi ([6c309f7](https://github.com/greenroom-robotics/mise/commit/6c309f75add8b5b5a02345f4ff94bafb2cafac30))
* add VincaBuildMode for build vinca's three valid flag combinations ([5e6f68b](https://github.com/greenroom-robotics/mise/commit/5e6f68b7719b599d1ccf13b1d5c6a815db8312ea))
* build matrix entries from state and pixi-native manifest ([92c1ac9](https://github.com/greenroom-robotics/mise/commit/92c1ac924facfc92b21bfcfb88d3beedf52961ca))
* classify changed paths into matrix state ([c793b4a](https://github.com/greenroom-robotics/mise/commit/c793b4a7c03e47223ddcb5a9f3318bd4eac4fe02))
* implement build deepstream-container as vinca delegation ([4ca3ee3](https://github.com/greenroom-robotics/mise/commit/4ca3ee3f59cebf91fe14fb50aa892138fdf2bc12))
* implement build pixi end-to-end ([e780c13](https://github.com/greenroom-robotics/mise/commit/e780c13f2a991ce70fc9e51aac361e9383cc7bf4))
* implement build vinca end-to-end ([6deefdd](https://github.com/greenroom-robotics/mise/commit/6deefdd486b167c0dcb98a8dc051105ffde04ed7))
* implement bump open-pr via git/gh CLI ([4fff73f](https://github.com/greenroom-robotics/mise/commit/4fff73f106042ede06295e024b36c4cba0ba40a3))
* implement bump pixi with comment-preserving yaml edit ([988095b](https://github.com/greenroom-robotics/mise/commit/988095b330def65fa8b06d6b9a93a585551ec6b5))
* implement bump recipe with comment-preserving yaml edit ([e8da2a2](https://github.com/greenroom-robotics/mise/commit/e8da2a2d8f85bd4da693f2e10c1866e5011d2f74))
* implement bump route via typed dispatch payload ([fa03aba](https://github.com/greenroom-robotics/mise/commit/fa03aba22eeb8d6069028d7a82b7621a5e1fe12f))
* implement matrix compute end-to-end ([ce69f49](https://github.com/greenroom-robotics/mise/commit/ce69f4946a528fab3766a30301220d6aafe4a0fe))
* implement snapshot refresh via ureq downloads ([e24e293](https://github.com/greenroom-robotics/mise/commit/e24e29375eeafe43f772836cce4f11090ecd8c79))
* make DeepstreamVersion orderable for deterministic matrix output ([f021dcc](https://github.com/greenroom-robotics/mise/commit/f021dcc906c3c87ae79802c7cc2099462dfb6516))
* scaffold CLI surface with typed newtypes and stubbed subcommands ([db5ac12](https://github.com/greenroom-robotics/mise/commit/db5ac128ca30a699d5f53f24b4075e21e12269b9))

### Bug Fixes

* load deepstream config from two files matching real layout ([9d8e2da](https://github.com/greenroom-robotics/mise/commit/9d8e2dada20c399ba64e3b89dd6136196817771f))
* reject newlines in outputs::set values ([0ff2f04](https://github.com/greenroom-robotics/mise/commit/0ff2f04a4e1b375920707a5420868d1752e3930b))
* tighten zero-SHA check and use correct git diff range for pushes ([daeb80e](https://github.com/greenroom-robotics/mise/commit/daeb80e75ffa910f6d20095b82c8f7323160d7db))

# Changelog

All notable changes to this project will be documented in this file.
This project uses semantic-release; entries are generated automatically from conventional commits.
