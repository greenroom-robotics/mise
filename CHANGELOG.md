## [4.4.0](https://github.com/greenroom-robotics/mise/compare/mise@4.3.0...mise@4.4.0) (2026-06-10)

### Features

* route pixi-native packages in ci recipes-pr ([#11](https://github.com/greenroom-robotics/mise/issues/11)) ([5a9ba95](https://github.com/greenroom-robotics/mise/commit/5a9ba95dc8f2398aaa55c00d70dcc79625f908fa))

## [4.3.0](https://github.com/greenroom-robotics/mise/compare/mise@4.2.0...mise@4.3.0) (2026-06-10)

### Features

* re-home vendored recipe handling into ci recipes-pr ([#10](https://github.com/greenroom-robotics/mise/issues/10)) ([dd1c22c](https://github.com/greenroom-robotics/mise/commit/dd1c22cc26c04677d24b9fdd73cbb40a67910cdf))

## [4.2.0](https://github.com/greenroom-robotics/mise/compare/mise@4.1.0...mise@4.2.0) (2026-06-09)

### Features

* bump vendored recipes from package-released dispatch ([#9](https://github.com/greenroom-robotics/mise/issues/9)) ([0705c7f](https://github.com/greenroom-robotics/mise/commit/0705c7fe597cd6194e89e21b9e5cf2fb2f541bcc))

## [4.1.0](https://github.com/greenroom-robotics/mise/compare/mise@4.0.0...mise@4.1.0) (2026-06-05)

### Features

* add skip-pixi input to setup action ([#8](https://github.com/greenroom-robotics/mise/issues/8)) ([574c7f1](https://github.com/greenroom-robotics/mise/commit/574c7f1736267a3be9a02259d9d5a03047d80b80))

## [4.0.0](https://github.com/greenroom-robotics/mise/compare/mise@3.0.0...mise@4.0.0) (2026-05-26)

### ⚠ BREAKING CHANGES

* **ci:** the ros-distro and recipes-repo inputs on the
build/test/release composite actions have been removed. Consumers
that set these inputs must remove them.

### Features

* **ci:** drop ros-distro/recipes-repo inputs from composite actions ([#7](https://github.com/greenroom-robotics/mise/issues/7)) ([c6650fe](https://github.com/greenroom-robotics/mise/commit/c6650fe85ef6f12d62fadb51130262bda98c8684))

## [3.0.0](https://github.com/greenroom-robotics/mise/compare/mise@2.3.0...mise@3.0.0) (2026-05-26)

### ⚠ BREAKING CHANGES

* **actions:** migrate to conda-channel-proxy v2 (multichannel) (#6)

### Features

* **actions:** migrate to conda-channel-proxy v2 (multichannel) ([#6](https://github.com/greenroom-robotics/mise/issues/6)) ([653e2f0](https://github.com/greenroom-robotics/mise/commit/653e2f0bb5bc0cbe97a9eb6c1d20aecbcd12e687))

## [2.3.0](https://github.com/greenroom-robotics/mise/compare/mise@2.2.1...mise@2.3.0) (2026-05-25)

### Features

* add ability to bump build epock ([e6fe4a4](https://github.com/greenroom-robotics/mise/commit/e6fe4a4ab80f0a3e033e117ad9a4dfcde1910e39))

## [2.2.1](https://github.com/greenroom-robotics/mise/compare/mise@2.2.0...mise@2.2.1) (2026-05-25)

### Bug Fixes

* **actions:** pin nested setup + docs to [@v2](https://github.com/v2) ([#5](https://github.com/greenroom-robotics/mise/issues/5)) ([e3050af](https://github.com/greenroom-robotics/mise/commit/e3050af9102f89e04fa9a388679ec20ba51908f6))

## [2.2.0](https://github.com/greenroom-robotics/mise/compare/mise@2.1.1...mise@2.2.0) (2026-05-25)

### Features

* ci release action ([#4](https://github.com/greenroom-robotics/mise/issues/4)) ([8a6f454](https://github.com/greenroom-robotics/mise/commit/8a6f454c94e231202b6e3eb6e0b53b8450db4bfa))

## [2.1.1](https://github.com/greenroom-robotics/mise/compare/mise@2.1.0...mise@2.1.1) (2026-05-25)

### Bug Fixes

* commit lockfile ([3b471b9](https://github.com/greenroom-robotics/mise/commit/3b471b9294f88abf44e3af7405ed1b22ed565de2))

## [2.1.0](https://github.com/greenroom-robotics/mise/compare/mise@2.0.0...mise@2.1.0) (2026-05-25)

### Features

* **ci:** add mise ci test/build and composite actions ([#3](https://github.com/greenroom-robotics/mise/issues/3)) ([fa04075](https://github.com/greenroom-robotics/mise/commit/fa04075791d77ccadc0a10f363175bb22356c4ff))

## [2.0.0](https://github.com/greenroom-robotics/mise/compare/mise@1.3.0...mise@2.0.0) (2026-05-22)

### ⚠ BREAKING CHANGES

* rename `mise build` to `mise build-recipes` (#2)

### Code Refactoring

* rename `mise build` to `mise build-recipes` ([#2](https://github.com/greenroom-robotics/mise/issues/2)) ([579e007](https://github.com/greenroom-robotics/mise/commit/579e007716eb8808e2c898d9a85cd71f1437eccb))

## [1.3.0](https://github.com/greenroom-robotics/mise/compare/mise@1.2.0...mise@1.3.0) (2026-05-22)

### Features

* require rev: and pre-flight pixi install --locked on pixi-native ([c3c6f19](https://github.com/greenroom-robotics/mise/commit/c3c6f19d5878e2d099d4488d5ae16a599416fb86))

## [1.2.0](https://github.com/greenroom-robotics/mise/compare/mise@1.1.0...mise@1.2.0) (2026-05-20)

### Features

* add --only flag to build vinca ([011ac36](https://github.com/greenroom-robotics/mise/commit/011ac361c6da76771130fc97ce8076a137a5826c))

## [1.1.0](https://github.com/greenroom-robotics/mise/compare/mise@1.0.3...mise@1.1.0) (2026-05-19)

### Features

* add parallel package checks ([83ae8ba](https://github.com/greenroom-robotics/mise/commit/83ae8ba7756999bed52bfb3dc7175fe1cb4987ae))

## [1.0.3](https://github.com/greenroom-robotics/mise/compare/mise@1.0.2...mise@1.0.3) (2026-05-19)

### Bug Fixes

* trigger ci ([0b62638](https://github.com/greenroom-robotics/mise/commit/0b62638df804a4ac8652f19406a54a41f7590188))

## [1.0.2](https://github.com/greenroom-robotics/mise/compare/mise@1.0.1...mise@1.0.2) (2026-05-19)

### Bug Fixes

* get proper build-number ([0256e66](https://github.com/greenroom-robotics/mise/commit/0256e66941e4da08c536368ac889ff47d00bc25f))

## [1.0.1](https://github.com/greenroom-robotics/mise/compare/mise@1.0.0...mise@1.0.1) (2026-05-19)

### Bug Fixes

* bump commands no longer absorb between-entry blank lines ([4775e8b](https://github.com/greenroom-robotics/mise/commit/4775e8be35605944fa979c67f6d9e38a2981853c))

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
