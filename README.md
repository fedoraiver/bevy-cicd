# Bevy CI/CD

供 Bevy 项目调用的 GitHub Actions 工作流。Rust 检查和 ECS 规则读取调用方的 Cargo 项目。ECS 架构工作流使用调用方的项目源码和应用导出的调度信息构建并校验静态站点，再由发布工作流部署到 GitHub Pages。

示例从 `main` 分支引用共享工作流。需要固定依赖时，将 `main` 替换为完整的提交 SHA。调用方须允许使用公开的 GitHub.com Actions 工作流。[GitHub 跨仓库复用说明](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows)

## Rust 检查

将下方配置存为调用仓库的 `.github/workflows/ci.yml`，将 `branches` 中的 `main` 改为项目默认分支。应用的 `Cargo.lock` 应提交到 Git。

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
    uses: fedoraiver/bevy-cicd/.github/workflows/rust-checks.yml@main
```

工作流从 `project-path`（默认 `.`）运行 Cargo，支持该路径或其上级目录声明的 rustup 工具链。未声明工具链时使用 `stable`。工作流为 Ubuntu 安装 Bevy 常见的窗口和音频编译依赖，依次运行格式检查、`cargo check`、Clippy 和串行测试。Clippy 开启 `pedantic` 警告。设置 `features`（例如 `debugdump` 或 `foo,bar`）会额外检查和测试该配置。

## Bevy 编译器规则

在上面的 `jobs` 增加下方任务即可启用。`package-path` 指向具体 Cargo 包，虚拟 workspace 可为不同成员分别调用。`features` 可指定额外配置。

```yaml
  bevy-rules:
    uses: fedoraiver/bevy-cicd/.github/workflows/bevy-rules.yml@main
    with:
      package-path: .
```

规则使用 `nightly-2026-10-31`，调用方的包及其锁定依赖须能用该工具链编译。回归覆盖 Bevy ECS 和 Scene 0.20.1。

- `entity-names` 检查生产代码创建的实体和子实体。初始组合必须提供非空的 Bevy `Name`，包括由 BSN 或 required component 提供的名称。无法确定名称的组合会报告诊断。
- `named-fields` 检查当前编译配置下项目 crate 中的单字段元组结构体，包括宏展开生成的类型。标记结构体、多字段元组结构体、枚举变体及依赖 crate 中的类型按原定义处理。

## 生成并发布 ECS 架构图

对于使用 Bevy 0.20 的 Cargo 包，在库根公开无参数的 `build_app`，复用游戏启动时的插件装配。下面的示例假定 `configure` 负责注册项目的真实插件。

```rust
pub fn build_app() -> bevy::prelude::App {
    let mut app = bevy::prelude::App::new();
    configure(&mut app);
    app
}
```

在项目的 `Cargo.toml` 声明 App 构造接口，并提交项目的 `Cargo.lock`。

```toml
[package.metadata.bevy-architecture]
app-factory = "build_app"
```

仓库根为虚拟 workspace 时，在根清单指定用于导出的成员包名。成员包清单仍声明自己的 App 构造函数。资源、锁文件和共享源码从调用方工作区解析。

```toml
[workspace.metadata.bevy-architecture]
package = "my-game"
```

共享工作流在自己的构建目录内生成启动器，通过 Cargo 路径依赖调用该函数，并在插件就绪后导出主应用和子应用的运行时记录与调度 DOT。旧项目可继续使用已有的 `debugdump` feature 和 `--dump-schedules <directory>` 入口。迁移时加入上面的构造函数和声明，再移除旧入口及其导出依赖。

在调用仓库的 **Settings → Pages** 将源设为 **GitHub Actions**。下面的工作流在 PR 中生成并校验站点，在默认分支合入后上线。将 `branches` 中的 `main` 改为项目实际的默认分支。

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
    uses: fedoraiver/bevy-cicd/.github/workflows/architecture-build.yml@main

  publish:
    needs: build
    if: ${{ github.event_name != 'pull_request' }}
    uses: fedoraiver/bevy-cicd/.github/workflows/architecture-pages.yml@main
    permissions:
      contents: read
      actions: read
      pages: write
      id-token: write
    with:
      artifact-name: ${{ needs.build.outputs.artifact-name }}
      verify-ecs-site: true
```

构建按固定 Rust 和 Node 依赖提取编译器事实、执行调度导出，并生成和检查 ECS 可视化站点。使用 App 构造函数的项目从同一轮编译取得项目库的事实和启动器二进制。运行证据以 `ecs-evidence-<run>-<attempt>` 产物保留。浏览和解释图中的证据见[ECS 架构图参考](tools/ecs-diagrams/README.md)，本地复现与排错见[维护指南](tools/ecs-diagrams/maintenance.md)。[共享调度导出启动器决策](docs/adr/0001-shared-schedule-launcher.md)

## 发布自定义架构站点

在调用仓库的 **Settings → Pages** 将发布源设为 **GitHub Actions**。调用方负责运行自己的生成和校验脚本，产物根目录应包含 `index.html`。下面的示例假设 `scripts/build-architecture.sh` 生成 `tmp/site` 目录。将 `branches` 中的 `main` 改为项目默认分支，按实际项目修改生成步骤。

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
      - uses: actions/checkout@main
        with:
          persist-credentials: false
      - name: Generate and validate site
        run: bash scripts/build-architecture.sh "$SITE"
      - name: Write publication proof
        run: |
          index_sha=$(sha256sum "$SITE/index.html" | cut -d ' ' -f 1)
          jq -n --arg commit "$GITHUB_SHA" --arg hash "$index_sha" \
            '{commit: $commit, index_sha256: $hash}' > "$SITE/bevy-cicd-build.json"
      - uses: actions/upload-pages-artifact@main
        with:
          name: ${{ env.ARCHITECTURE_ARTIFACT }}
          path: ${{ env.SITE }}
          retention-days: 3
      - uses: actions/upload-artifact@main
        with:
          name: ${{ env.ARCHITECTURE_ARTIFACT }}-proof
          path: ${{ env.SITE }}/bevy-cicd-build.json
          if-no-files-found: error
          retention-days: 3

  publish:
    needs: build
    uses: fedoraiver/bevy-cicd/.github/workflows/architecture-pages.yml@main
    permissions:
      contents: read
      actions: read
      pages: write
      id-token: write
    with:
      artifact-name: ${{ needs.build.outputs.artifact-name }}
```

发布任务从同一运行下载证明，并在部署前核对待发布提交是不是默认分支的最新提交。发布后通过公开 HTTP 地址核对证明及首页哈希。后续任务可以从 `needs.publish.outputs.page_url` 读取验证通过的地址。局部重跑使用原构建任务输出的产物名，受产物保留期限制。[GitHub Pages 官方说明](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)
