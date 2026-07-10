## [5.12.0](https://github.com/greenroom-robotics/mise/compare/mise@5.11.0...mise@5.12.0) (2026-07-10)


### Features

* **ci-test:** add host-setup input for pre-test host provisioning ([#49](https://github.com/greenroom-robotics/mise/issues/49)) ([3375b76](https://github.com/greenroom-robotics/mise/commit/3375b7615784688f39520f14148d0c3d64960067))

## [5.11.0](https://github.com/greenroom-robotics/mise/compare/mise@5.10.0...mise@5.11.0) (2026-07-03)


### Features

* **recipes-pr:** link source diff since last release in PR body ([#48](https://github.com/greenroom-robotics/mise/issues/48)) ([459a8af](https://github.com/greenroom-robotics/mise/commit/459a8af164c6edb842a3a83075512df5c5400256))

## [5.10.0](https://github.com/greenroom-robotics/mise/compare/mise@5.9.0...mise@5.10.0) (2026-07-03)


### Features

* build noarch packages only on linux-64 ([#47](https://github.com/greenroom-robotics/mise/issues/47)) ([80b784e](https://github.com/greenroom-robotics/mise/commit/80b784e651e17876ad25f3764c9d7263cef158cf))

## [5.9.0](https://github.com/greenroom-robotics/mise/compare/mise@5.8.4...mise@5.9.0) (2026-06-29)


### Features

* **ci-test:** add exclude-packages input to drop packages from discovery ([#46](https://github.com/greenroom-robotics/mise/issues/46)) ([e18deff](https://github.com/greenroom-robotics/mise/commit/e18deff074ded4d41283abf64c36545cfbd5c4a8))

## [5.8.4](https://github.com/greenroom-robotics/mise/compare/mise@5.8.3...mise@5.8.4) (2026-06-29)


### Bug Fixes

* **ci-test:** pin LFS pull to workspace and mark safe.directory ([#45](https://github.com/greenroom-robotics/mise/issues/45)) ([c9fed74](https://github.com/greenroom-robotics/mise/commit/c9fed74044cf6bc131b9df3ad17b1e2258fedf83))

## [5.8.3](https://github.com/greenroom-robotics/mise/compare/mise@5.8.2...mise@5.8.3) (2026-06-29)


### Bug Fixes

* **ci-test:** install gh (and base tooling) in container job ([#44](https://github.com/greenroom-robotics/mise/issues/44)) ([ffb4c5a](https://github.com/greenroom-robotics/mise/commit/ffb4c5a640ef81217741c50e13a2e94fcb9276a8))

## [5.8.2](https://github.com/greenroom-robotics/mise/compare/mise@5.8.1...mise@5.8.2) (2026-06-29)


### Bug Fixes

* **ci-test:** pull all LFS objects in container job, overriding fetchexclude ([#43](https://github.com/greenroom-robotics/mise/issues/43)) ([f9b7721](https://github.com/greenroom-robotics/mise/commit/f9b772188e00d1f8a0f3afe5a4d4975ff0cc8cdc))

## [5.8.1](https://github.com/greenroom-robotics/mise/compare/mise@5.8.0...mise@5.8.1) (2026-06-29)


### Bug Fixes

* **ci-test:** install git-lfs in container job when lfs enabled ([#42](https://github.com/greenroom-robotics/mise/issues/42)) ([406246e](https://github.com/greenroom-robotics/mise/commit/406246ef4caee99de1f6bb56f2604517051bcef9))

## [5.8.0](https://github.com/greenroom-robotics/mise/compare/mise@5.7.2...mise@5.8.0) (2026-06-29)


### Features

* **ci-test:** run selected packages' tests in a container on a separate runner ([#41](https://github.com/greenroom-robotics/mise/issues/41)) ([da18e10](https://github.com/greenroom-robotics/mise/commit/da18e109b8d5de2208469b8acb79f3f6ac9613c2))

## [5.7.2](https://github.com/greenroom-robotics/mise/compare/mise@5.7.1...mise@5.7.2) (2026-06-29)


### Bug Fixes

* trigger release ([af8bd40](https://github.com/greenroom-robotics/mise/commit/af8bd400acf05a2548bffa49b0716122890474be))

## [5.7.1](https://github.com/greenroom-robotics/mise/compare/mise@5.7.0...mise@5.7.1) (2026-06-29)


### Bug Fixes

* **ci-test:** replace runners map with gpu-runner/gpu-packages ([#39](https://github.com/greenroom-robotics/mise/issues/39)) ([6bdaa55](https://github.com/greenroom-robotics/mise/commit/6bdaa5574f104164833f8746b0531217d6a77c7e))

## [5.7.0](https://github.com/greenroom-robotics/mise/compare/mise@5.6.1...mise@5.7.0) (2026-06-29)


### Features

* **ci-test:** per-package runner override map ([#37](https://github.com/greenroom-robotics/mise/issues/37)) ([3c2db4d](https://github.com/greenroom-robotics/mise/commit/3c2db4dc17489e38a6770575c49c6f6c62076a54))

## [5.6.1](https://github.com/greenroom-robotics/mise/compare/mise@5.6.0...mise@5.6.1) (2026-06-28)


### Bug Fixes

* add back extra flags ([443ae1c](https://github.com/greenroom-robotics/mise/commit/443ae1ce1973df34bcb27aa68f9e484d4b04353d))

## [5.6.0](https://github.com/greenroom-robotics/mise/compare/mise@5.5.0...mise@5.6.0) (2026-06-28)


### Features

* fold cargo bump into release commit via extra prepare/assets ([2e7a8ed](https://github.com/greenroom-robotics/mise/commit/2e7a8ed4971b68ae626d6a997415917f94c76fb9))
* trigger release ([091ed9d](https://github.com/greenroom-robotics/mise/commit/091ed9d7d00f5ea9f5e76e54f380289b99edf8be))


### Bug Fixes

* temporarily drop unpublished commands ([874ea96](https://github.com/greenroom-robotics/mise/commit/874ea96614e904d0a4de146f2575297a9f987f1d))

## [5.5.0](https://github.com/greenroom-robotics/mise/compare/mise@5.4.0...mise@5.5.0) (2026-06-28)


### Features

* add changed-only and lfs inputs to ci-test matrix workflow ([c0d6918](https://github.com/greenroom-robotics/mise/commit/c0d691884fbc5e20198c4222189fb21ca49e59fc))

## [5.4.0](https://github.com/greenroom-robotics/mise/compare/mise@5.3.0...mise@5.4.0) (2026-06-28)


### Features

* add matrix reusable workflow for per-package ci test ([2cad617](https://github.com/greenroom-robotics/mise/commit/2cad617a84f3a9352bdffdce4598568a4ed12dfa))


### Bug Fixes

* bump build backend ([562eba0](https://github.com/greenroom-robotics/mise/commit/562eba06e2855484adf672f8d6bcc405b47618eb))

## [5.3.0](https://github.com/greenroom-robotics/mise/compare/mise@5.2.2...mise@5.3.0) (2026-06-28)


### Features

* support single-package repos in recipes-pr ([#36](https://github.com/greenroom-robotics/mise/issues/36)) ([933b40a](https://github.com/greenroom-robotics/mise/commit/933b40a1690ab7ce29b5449d5112263bb5b06d24))

## [5.2.2](https://github.com/greenroom-robotics/mise/compare/mise@5.2.1...mise@5.2.2) (2026-06-28)


### Bug Fixes

* pin internal setup refs to v5 and guard major drift ([28ecf24](https://github.com/greenroom-robotics/mise/commit/28ecf24c184dd2463540c827e37f7118684e3dd3))

## [5.2.1](https://github.com/greenroom-robotics/mise/compare/mise@5.2.0...mise@5.2.1) (2026-06-28)


### Bug Fixes

* properly handle superseded publish workflows ([1e4132a](https://github.com/greenroom-robotics/mise/commit/1e4132ab8af3c9a5065b43253905dcf8935e9516))

## [5.2.0](https://github.com/greenroom-robotics/mise/compare/mise@5.1.0...mise@5.2.0) (2026-06-27)


### Features

* bump version ([0dbfc33](https://github.com/greenroom-robotics/mise/commit/0dbfc3364a3ccdb9f35278b6c884bd2a5ab87369))

## [5.1.0](https://github.com/greenroom-robotics/mise/compare/mise@5.0.0...mise@5.1.0) (2026-06-27)


### Features

* bump version ([b0a4c8d](https://github.com/greenroom-robotics/mise/commit/b0a4c8ddf7c7e57c1c5e91439af011245d8de1b7))

## [5.0.0](https://github.com/greenroom-robotics/mise/compare/mise@4.13.0...mise@5.0.0) (2026-06-27)


### ⚠ BREAKING CHANGES

* **setup:** pixi >0.70 is not backwards compatible with old build backends

### Features

* **setup:** bump pixi to 0.71.1 ([fd86aba](https://github.com/greenroom-robotics/mise/commit/fd86abad32f847a47c6ca564b05c1e7a0b00e8fb))

## [4.13.0](https://github.com/greenroom-robotics/mise/compare/mise@4.12.0...mise@4.13.0) (2026-06-26)


### Features

* add lock-check action and mise ci lock-check ([#33](https://github.com/greenroom-robotics/mise/issues/33)) ([405082e](https://github.com/greenroom-robotics/mise/commit/405082edc3b4460043d11451c26ce1e887889c7f))

## [4.12.0](https://github.com/greenroom-robotics/mise/compare/mise@4.11.1...mise@4.12.0) (2026-06-26)


### Features

* add gremlin messages ([#32](https://github.com/greenroom-robotics/mise/issues/32)) ([11cad09](https://github.com/greenroom-robotics/mise/commit/11cad09cd9d92e8ccb4a9571c0ebed4b23de2b6a))

## [4.11.1](https://github.com/greenroom-robotics/mise/compare/mise@4.11.0...mise@4.11.1) (2026-06-22)


### Bug Fixes

* **ci:** also exclude package.xml from collected reports ([#30](https://github.com/greenroom-robotics/mise/issues/30)) ([c203702](https://github.com/greenroom-robotics/mise/commit/c20370264e093dbca184f5a2370731866a3e0e45))

## [4.11.0](https://github.com/greenroom-robotics/mise/compare/mise@4.10.1...mise@4.11.0) (2026-06-22)


### Features

* recipes-pr composite action for conda publish off deb tags ([#31](https://github.com/greenroom-robotics/mise/issues/31)) ([af2c59e](https://github.com/greenroom-robotics/mise/commit/af2c59e0ca3ed8903ddb1edf264a23a5cfafbf37))

## [4.10.1](https://github.com/greenroom-robotics/mise/compare/mise@4.10.0...mise@4.10.1) (2026-06-22)


### Bug Fixes

* **ci:** exclude CTest Test.xml from collected reports ([#29](https://github.com/greenroom-robotics/mise/issues/29)) ([d927701](https://github.com/greenroom-robotics/mise/commit/d927701d4a4d04dd3e023d700d3e92ce352d2e25))

## [4.10.0](https://github.com/greenroom-robotics/mise/compare/mise@4.9.1...mise@4.10.0) (2026-06-19)


### Features

* use package name for release pr branches ([f70de9d](https://github.com/greenroom-robotics/mise/commit/f70de9d76b1cc3cd828ca0fea18a825098fb883e))

## [4.9.1](https://github.com/greenroom-robotics/mise/compare/mise@4.9.0...mise@4.9.1) (2026-06-19)


### Bug Fixes

* use --locked for test runs ([344ac5b](https://github.com/greenroom-robotics/mise/commit/344ac5b21784f6c2fa981c31181b096c58f9efd2))

## [4.9.0](https://github.com/greenroom-robotics/mise/compare/mise@4.8.1...mise@4.9.0) (2026-06-19)


### Features

* add ability to specify extra test tasks ([#28](https://github.com/greenroom-robotics/mise/issues/28)) ([2890ab2](https://github.com/greenroom-robotics/mise/commit/2890ab2437699e8d41b94239351b35ce1582dc4f))

## [4.8.1](https://github.com/greenroom-robotics/mise/compare/mise@4.8.0...mise@4.8.1) (2026-06-12)


### Bug Fixes

* update for new pixi version ([86312e6](https://github.com/greenroom-robotics/mise/commit/86312e6501c70a095757bb02bc550708dce68a4f))

## [4.8.0](https://github.com/greenroom-robotics/mise/compare/mise@4.7.0...mise@4.8.0) (2026-06-12)


### Features

* collect junit test reports from ci test ([#26](https://github.com/greenroom-robotics/mise/issues/26)) ([e345497](https://github.com/greenroom-robotics/mise/commit/e345497ccf8ac677391940e821acc321934c0915))

## [4.7.0](https://github.com/greenroom-robotics/mise/compare/mise@4.6.0...mise@4.7.0) (2026-06-11)


### Features

* use github identity from gh-token ([#25](https://github.com/greenroom-robotics/mise/issues/25)) ([77d949b](https://github.com/greenroom-robotics/mise/commit/77d949bc7266751b7ef460bbc64451774e1fb1e4))

## [4.6.0](https://github.com/greenroom-robotics/mise/compare/mise@4.5.3...mise@4.6.0) (2026-06-11)


### Features

* diff-aware pixi-native builds + runner-size pruning ([#24](https://github.com/greenroom-robotics/mise/issues/24)) ([c8290fd](https://github.com/greenroom-robotics/mise/commit/c8290fd2738a8708772876191a611005070a61b2))

## [4.5.3](https://github.com/greenroom-robotics/mise/compare/mise@4.5.2...mise@4.5.3) (2026-06-11)


### Bug Fixes

* use rolling version-independent branch for recipes-pr ([#22](https://github.com/greenroom-robotics/mise/issues/22)) ([ad076be](https://github.com/greenroom-robotics/mise/commit/ad076be208edd7831e7b5c001cd7f4c1d5919438))

## [4.5.2](https://github.com/greenroom-robotics/mise/compare/mise@4.5.1...mise@4.5.2) (2026-06-11)


### Bug Fixes

* bump conda-channel-proxy to v0.5.1 (defaults to dev azure creds on RunsOn) ([#20](https://github.com/greenroom-robotics/mise/issues/20)) ([ba1f807](https://github.com/greenroom-robotics/mise/commit/ba1f8073d00f8fc0cc4ea7d03481db36c6ffa4be))

## [4.5.1](https://github.com/greenroom-robotics/mise/compare/mise@4.5.0...mise@4.5.1) (2026-06-11)


### Bug Fixes

* bump conda-channel-proxy to v3/v0.5.0 (port 12222), pin internal action refs to v4 ([#18](https://github.com/greenroom-robotics/mise/issues/18)) ([ff9701b](https://github.com/greenroom-robotics/mise/commit/ff9701b4b9da830a73cd67b9cd1df6a7f22bf2da))
* pin pixi-build-rust backend, 2026-06-10 builds break pixi 0.68-gr1 ([#19](https://github.com/greenroom-robotics/mise/issues/19)) ([4f8b9d5](https://github.com/greenroom-robotics/mise/commit/4f8b9d520681725a746ba1b5b0fd48d4f552aeb8))

## [4.5.0](https://github.com/greenroom-robotics/mise/compare/mise@4.4.0...mise@4.5.0) (2026-06-10)


### Features

* trigger CI ([6753b77](https://github.com/greenroom-robotics/mise/commit/6753b77c83a88b18a053c0997347f6b0e8fc8195))


### Bug Fixes

* ci release --changelog/--github-release accept explicit bool value ([#16](https://github.com/greenroom-robotics/mise/issues/16)) ([a2580a0](https://github.com/greenroom-robotics/mise/commit/a2580a0946545444e34cb824554014cb6eb04267))
* dogfood in-repo setup in release workflow to break v-tag deadlock ([#15](https://github.com/greenroom-robotics/mise/issues/15)) ([cdca58f](https://github.com/greenroom-robotics/mise/commit/cdca58faa36e52fabaa6753f9f7a8a01ebe0fcbf))
* release tag format and recipes PR auto-merge ([#17](https://github.com/greenroom-robotics/mise/issues/17)) ([c759e3d](https://github.com/greenroom-robotics/mise/commit/c759e3d822fedce0684d81231d71d378f8188b85))
* scope app token to installation owner for private proxy access ([#14](https://github.com/greenroom-robotics/mise/issues/14)) ([1563cdf](https://github.com/greenroom-robotics/mise/commit/1563cdf40efa12f0f181dcbc9309fbed058ed1e9))

## 1.0.0 (2026-06-10)


### ⚠ BREAKING CHANGES

* **ci:** the ros-distro and recipes-repo inputs on the
build/test/release composite actions have been removed. Consumers
that set these inputs must remove them.
* **actions:** migrate to conda-channel-proxy v2 (multichannel) (#6)
* rename `mise build` to `mise build-recipes` (#2)

### Features

* **actions:** migrate to conda-channel-proxy v2 (multichannel) ([#6](https://github.com/greenroom-robotics/mise/issues/6)) ([653e2f0](https://github.com/greenroom-robotics/mise/commit/653e2f0bb5bc0cbe97a9eb6c1d20aecbcd12e687))
* add --only flag to build vinca ([011ac36](https://github.com/greenroom-robotics/mise/commit/011ac361c6da76771130fc97ce8076a137a5826c))
* add --repo-root to bump and snapshot subcommands ([35a88b9](https://github.com/greenroom-robotics/mise/commit/35a88b99bc896550fa0d17942529e671a44821f6))
* add --runner-size flag to build pixi ([f0adfea](https://github.com/greenroom-robotics/mise/commit/f0adfeae82b000d08a73d3f9250745b040cffd4b))
* add ability to bump build epock ([e6fe4a4](https://github.com/greenroom-robotics/mise/commit/e6fe4a4ab80f0a3e033e117ad9a4dfcde1910e39))
* add parallel package checks ([83ae8ba](https://github.com/greenroom-robotics/mise/commit/83ae8ba7756999bed52bfb3dc7175fe1cb4987ae))
* add Pipeline and MatrixEntry types for build matrix ([f2ae7a1](https://github.com/greenroom-robotics/mise/commit/f2ae7a1cd885840314d9ef7227a38e446f332a96))
* add recipe-dir filter and variants-pin helpers for build vinca ([3adffc8](https://github.com/greenroom-robotics/mise/commit/3adffc84464cbc4faf33195b69ab216910e99b7d))
* add Repo discovery and typed YAML loaders ([aca9a80](https://github.com/greenroom-robotics/mise/commit/aca9a80cc62afd47ce46fc60548a74afabc30000))
* add skip-pixi input to setup action ([#8](https://github.com/greenroom-robotics/mise/issues/8)) ([574c7f1](https://github.com/greenroom-robotics/mise/commit/574c7f1736267a3be9a02259d9d5a03047d80b80))
* add subprocess helpers with stderr-on-error context ([2330c7d](https://github.com/greenroom-robotics/mise/commit/2330c7d402cb66c9eaa855194550c2bb9255c8a6))
* add typed GitHub Actions event parsing and outputs helper ([88a7fae](https://github.com/greenroom-robotics/mise/commit/88a7fae523d9de51878253743ea309d325a645b2))
* add upstream pixi.toml parser for build pixi ([6c309f7](https://github.com/greenroom-robotics/mise/commit/6c309f75add8b5b5a02345f4ff94bafb2cafac30))
* add VincaBuildMode for build vinca's three valid flag combinations ([5e6f68b](https://github.com/greenroom-robotics/mise/commit/5e6f68b7719b599d1ccf13b1d5c6a815db8312ea))
* build matrix entries from state and pixi-native manifest ([92c1ac9](https://github.com/greenroom-robotics/mise/commit/92c1ac924facfc92b21bfcfb88d3beedf52961ca))
* bump vendored recipes from package-released dispatch ([#9](https://github.com/greenroom-robotics/mise/issues/9)) ([0705c7f](https://github.com/greenroom-robotics/mise/commit/0705c7fe597cd6194e89e21b9e5cf2fb2f541bcc))
* ci release action ([#4](https://github.com/greenroom-robotics/mise/issues/4)) ([8a6f454](https://github.com/greenroom-robotics/mise/commit/8a6f454c94e231202b6e3eb6e0b53b8450db4bfa))
* **ci:** add mise ci test/build and composite actions ([#3](https://github.com/greenroom-robotics/mise/issues/3)) ([fa04075](https://github.com/greenroom-robotics/mise/commit/fa04075791d77ccadc0a10f363175bb22356c4ff))
* **ci:** drop ros-distro/recipes-repo inputs from composite actions ([#7](https://github.com/greenroom-robotics/mise/issues/7)) ([c6650fe](https://github.com/greenroom-robotics/mise/commit/c6650fe85ef6f12d62fadb51130262bda98c8684))
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
* re-home vendored recipe handling into ci recipes-pr ([#10](https://github.com/greenroom-robotics/mise/issues/10)) ([dd1c22c](https://github.com/greenroom-robotics/mise/commit/dd1c22cc26c04677d24b9fdd73cbb40a67910cdf))
* require rev: and pre-flight pixi install --locked on pixi-native ([c3c6f19](https://github.com/greenroom-robotics/mise/commit/c3c6f19d5878e2d099d4488d5ae16a599416fb86))
* route pixi-native packages in ci recipes-pr ([#11](https://github.com/greenroom-robotics/mise/issues/11)) ([5a9ba95](https://github.com/greenroom-robotics/mise/commit/5a9ba95dc8f2398aaa55c00d70dcc79625f908fa))
* scaffold CLI surface with typed newtypes and stubbed subcommands ([db5ac12](https://github.com/greenroom-robotics/mise/commit/db5ac128ca30a699d5f53f24b4075e21e12269b9))
* trigger CI ([6753b77](https://github.com/greenroom-robotics/mise/commit/6753b77c83a88b18a053c0997347f6b0e8fc8195))


### Bug Fixes

* **actions:** pin nested setup + docs to [@v2](https://github.com/v2) ([#5](https://github.com/greenroom-robotics/mise/issues/5)) ([e3050af](https://github.com/greenroom-robotics/mise/commit/e3050af9102f89e04fa9a388679ec20ba51908f6))
* bump commands no longer absorb between-entry blank lines ([4775e8b](https://github.com/greenroom-robotics/mise/commit/4775e8be35605944fa979c67f6d9e38a2981853c))
* ci release --changelog/--github-release accept explicit bool value ([#16](https://github.com/greenroom-robotics/mise/issues/16)) ([a2580a0](https://github.com/greenroom-robotics/mise/commit/a2580a0946545444e34cb824554014cb6eb04267))
* commit lockfile ([3b471b9](https://github.com/greenroom-robotics/mise/commit/3b471b9294f88abf44e3af7405ed1b22ed565de2))
* dogfood in-repo setup in release workflow to break v-tag deadlock ([#15](https://github.com/greenroom-robotics/mise/issues/15)) ([cdca58f](https://github.com/greenroom-robotics/mise/commit/cdca58faa36e52fabaa6753f9f7a8a01ebe0fcbf))
* get proper build-number ([0256e66](https://github.com/greenroom-robotics/mise/commit/0256e66941e4da08c536368ac889ff47d00bc25f))
* load deepstream config from two files matching real layout ([9d8e2da](https://github.com/greenroom-robotics/mise/commit/9d8e2dada20c399ba64e3b89dd6136196817771f))
* reject newlines in outputs::set values ([0ff2f04](https://github.com/greenroom-robotics/mise/commit/0ff2f04a4e1b375920707a5420868d1752e3930b))
* scope app token to installation owner for private proxy access ([#14](https://github.com/greenroom-robotics/mise/issues/14)) ([1563cdf](https://github.com/greenroom-robotics/mise/commit/1563cdf40efa12f0f181dcbc9309fbed058ed1e9))
* tighten zero-SHA check and use correct git diff range for pushes ([daeb80e](https://github.com/greenroom-robotics/mise/commit/daeb80e75ffa910f6d20095b82c8f7323160d7db))
* trigger ci ([0b62638](https://github.com/greenroom-robotics/mise/commit/0b62638df804a4ac8652f19406a54a41f7590188))


### Code Refactoring

* rename `mise build` to `mise build-recipes` ([#2](https://github.com/greenroom-robotics/mise/issues/2)) ([579e007](https://github.com/greenroom-robotics/mise/commit/579e007716eb8808e2c898d9a85cd71f1437eccb))

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
