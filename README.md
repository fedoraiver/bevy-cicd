# Bevy CI/CD

供 Bevy 项目调用的 GitHub Actions 工作流。Rust 检查和 ECS 规则读取调用方的 Cargo 项目，架构发布使用调用方已经构建的静态站点。

示例使用发布标签 `@v1.0.0`。需要固定依赖时，将标签替换为对应的完整提交 SHA。调用方须允许使用公开的 GitHub.com Actions 工作流。[GitHub 跨仓库复用说明](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows)

## Rust 检查

将下方配置存为调用仓库的 `.github/workflows/ci.yml`，把 `main` 改为项目默认分支。应用的 `Cargo.lock` 应提交到 Git。

```yaml
name: Rust CI
on:
  pull_request:
    branches: [main]
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read

jobs:
  rust:
    uses: fedoraiver/bevy-cicd/.github/workflows/rust-checks.yml@v1.0.0
```

工作流从 `project-path`（默认 `.`）运行 Cargo，支持该路径或其上级目录声明的 rustup 工具链。未声明工具链时使用 `stable`。工作流为 Ubuntu 安装 Bevy 常见的窗口和音频编译依赖，依次运行格式检查、`cargo check`、Clippy 和串行测试。Clippy 开启 `pedantic` 警告。设置 `features`（例如 `debugdump` 或 `foo,bar`）会额外检查和测试该配置。

## Bevy 编译器规则

在上面的 `jobs` 增加下方任务即可启用。`package-path` 指向具体 Cargo 包，虚拟 workspace 可为不同成员分别调用。`features` 可指定额外配置。

```yaml
  bevy-rules:
    uses: fedoraiver/bevy-cicd/.github/workflows/bevy-rules.yml@v1.0.0
    with:
      package-path: .
```

规则检查生产代码创建的 Bevy 实体是否具有初始名称，以及 Rust 单字段元组结构体等声明是否使用了具名字段。分析器固定使用 `nightly-2026-10-31`，调用方的 Cargo 包及其锁定依赖须能用该工具链编译。仓库回归覆盖 Bevy ECS 和 Scene 0.20.1。

## 生成并发布 ECS 架构图

把项目的 App 装配复用于 `--dump-schedules <directory>` 入口，并通过 `debugdump` feature 启用导出。下面的 Cargo 配置适用于 Bevy 0.20。合并到项目现有的 `[features]` 与 `[dependencies]`，并提交更新后的 `Cargo.lock`。

```toml
[features]
debugdump = ["dep:bevy-schedule-export", "bevy/debug"]

[dependencies]
bevy-schedule-export = { git = "https://github.com/fedoraiver/bevy-cicd.git", tag = "v1.1.0", optional = true }
```

导出入口注册和装配项目的实际插件，调用下面的函数后退出。正常启动仍使用项目自己的运行入口。导出函数写入主世界及子世界的运行时记录和调度 DOT。

```rust
bevy_schedule_export::dump_schedules(
    &mut app,
    &output_directory,
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
)?;
```

在调用仓库的 **Settings → Pages** 将源设为 **GitHub Actions**。下面的工作流在 PR 中生成并校验站点，在默认分支合入后上线。将 `main` 改为项目实际的默认分支。

```yaml
name: ECS architecture
on:
  pull_request:
    branches: [main]
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read

jobs:
  build:
    uses: fedoraiver/bevy-cicd/.github/workflows/architecture-build.yml@v1.1.0

  publish:
    needs: build
    if: ${{ github.event_name != 'pull_request' }}
    uses: fedoraiver/bevy-cicd/.github/workflows/architecture-pages.yml@v1.1.0
    permissions:
      contents: read
      actions: read
      pages: write
      id-token: write
    with:
      artifact-name: ${{ needs.build.outputs.artifact-name }}
```

构建按固定 Rust 和 Node 依赖分析调用仓库根目录的 Cargo 二进制，提取编译器事实，执行调度导出，生成并检查 ECS 可视化站点。运行证据以 `ecs-evidence-<run>-<attempt>` 产物保留。

## 发布自定义架构站点

在调用仓库的 **Settings → Pages** 将发布源设为 **GitHub Actions**。调用方负责运行自己的生成和校验脚本，产物根目录应包含 `index.html`。下面的示例假设 `scripts/build-architecture.sh` 生成 `tmp/site` 目录。把 `main` 改为项目默认分支，按实际项目修改生成步骤。

```yaml
name: Publish architecture
on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read

jobs:
  build:
    runs-on: ubuntu-latest
    outputs:
      artifact-name: ${{ env.ARCHITECTURE_ARTIFACT }}
    env:
      ARCHITECTURE_ARTIFACT: architecture-${{ github.run_id }}-${{ github.run_attempt }}
      SITE: tmp/site
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7
        with:
          persist-credentials: false
      - name: Generate and validate site
        run: bash scripts/build-architecture.sh "$SITE"
      - name: Write publication proof
        run: |
          index_sha=$(sha256sum "$SITE/index.html" | cut -d ' ' -f 1)
          jq -n --arg commit "$GITHUB_SHA" --arg hash "$index_sha" \
            '{commit: $commit, index_sha256: $hash}' > "$SITE/bevy-cicd-build.json"
      - uses: actions/upload-pages-artifact@7b1f4a764d45c48632c6b24a0339c27f5614fb0b # v4
        with:
          name: ${{ env.ARCHITECTURE_ARTIFACT }}
          path: ${{ env.SITE }}
          retention-days: 3
      - uses: actions/upload-artifact@b7c566a772e6b6bfb58ed0dc250532a479d7789f # v6
        with:
          name: ${{ env.ARCHITECTURE_ARTIFACT }}-proof
          path: ${{ env.SITE }}/bevy-cicd-build.json
          if-no-files-found: error
          retention-days: 3

  publish:
    needs: build
    uses: fedoraiver/bevy-cicd/.github/workflows/architecture-pages.yml@v1.0.0
    permissions:
      contents: read
      actions: read
      pages: write
      id-token: write
    with:
      artifact-name: ${{ needs.build.outputs.artifact-name }}
```

发布任务从同一运行下载证明，并在部署前核对待发布提交是不是默认分支的最新提交。发布后通过公开 HTTP 地址核对证明及首页哈希。后续任务可以从 `needs.publish.outputs.page_url` 读取验证通过的地址。局部重跑使用原构建任务输出的产物名，受产物保留期限制。[GitHub Pages 官方说明](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)
