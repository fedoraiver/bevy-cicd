# ECS 架构图维护

通过调用方的 GitHub Actions 工作流生成、校验和发布正式站点。[工作流接入](../workflows.md#生成并发布-ecs-架构图) · [模型与查看器](README.md)

## 调度导出边界

共享启动器调用项目库的 `build_app`，插件就绪后读取主应用和子应用的调度。启动器与项目库在同一轮编译中构建，运行时类型只绑定至该轮事实。导出发生在 `finish` 之后、应用循环之前，期间可能初始化 GPU、音频设备和资产。官方构建使用 Xvfb 虚拟显示与 Mesa Lavapipe 软件 Vulkan。已配置的调用方日志和导出结果随构建证据保留。

架构站描述当前源码、依赖和原生构建目标下的注册结构。应用运行后新增的调度和其他构建配置需要独立检查。

## 工作流故障排查

在 Linux 准备 Bash、Node 24、共享仓库锁定的 Rust 工具链、Bevy 构建依赖、Xvfb、软件 Vulkan 与中文字体。参照[构建工作流](../../.github/workflows/architecture-build.yml)安装系统依赖和锁定浏览器。视觉回归同时读取 `MERMAID_CHROMIUM_PATH` 和 `MAINTENANCE_VISUAL_CHROMIUM`。字体环境变化会使相应的布局缓存失效。

从 bevy-cicd 根目录运行以下命令。将 `BEVY_ARCHITECTURE_WORKSPACE` 设为目标项目 checkout 的绝对路径，工具和图包使用同一 bevy-cicd 修订。

```bash
export BEVY_ARCHITECTURE_WORKSPACE=/absolute/path/to/project
export PUPPETEER_CACHE_DIR="$BEVY_ARCHITECTURE_WORKSPACE/tmp/ecs-architecture/puppeteer"
export WGPU_BACKEND=vulkan WGPU_FORCE_FALLBACK_ADAPTER=1
bash scripts/rustc-analysis.sh prepare-architecture "$BEVY_ARCHITECTURE_WORKSPACE"
npm ci --foreground-scripts --prefix tools/ecs-diagrams
npm exec --prefix tools/ecs-diagrams --yes=false -- puppeteer browsers install chrome
export MERMAID_CHROMIUM_PATH="$(cd tools/ecs-diagrams &&
  node --input-type=module -e 'import puppeteer from "puppeteer"; console.log(await puppeteer.executablePath());')"
export MAINTENANCE_VISUAL_CHROMIUM="$MERMAID_CHROMIUM_PATH"
node tools/ecs-diagrams/maintain.mjs plan
xvfb-run -a node tools/ecs-diagrams/maintain.mjs run
node tools/ecs-diagrams/maintain.mjs pack-pages
node tools/ecs-diagrams/maintain.mjs check-pages
```

`plan` 核对输入和已有证据。`run` 提取事实和真实 Schedule、装配模型、生成站点、执行审计与回归。任一阶段失败时，先用 `status --run-id RUN_ID` 或 `report --format markdown` 找到失败阶段及原始日志。修复后使用 `run --resume RUN_ID` 重试，仍匹配输入、工具和校验结果的已完成阶段可以复用。

调用方的 `tmp/ecs-architecture/maintenance/<run-id>/` 保存阶段状态和日志，`tmp/ecs-architecture/compiler/` 保存事实与快照。工具 checkout 的 `tmp/rustc-analysis/` 保存编译器工具产物，`tmp/ra/r/` 保存编译与报告绑定日志。工作区和原生编译缓存各有运行锁，使用同一 checkout 时串行执行构建。[构建缓存与完整性](README.md#构建缓存与完整性)

`preflight --debugdump PATH` 单独核对冻结模型与调度快照。`pack-pages --out PATH` 生成静态包，`check-pages --directory PATH` 核对静态包。静态包默认位于调用方的 `tmp/ecs-architecture/pages/`，相对路径按调用方根目录解析。

启用 `verify-ecs-site` 的 Pages 发布使用同轮构建上传的站点清单校验已上线的提交、页面和代表性资源。HTTP 核对记录保存在 `ecs-deployment-<run>-<attempt>` 产物的 `verification.json`。

## 完整审计与 Pages 静态包

`tmp/ecs-architecture/generated/` 保存完整审计输出，不纳入 Git，也不使用 Git LFS。完整清单核验 schema `2` 模型、全部调用图 NDJSON、冻结源码、对象身份、关系证据及真实 Schedule；缺少调用图、哈希或准确边数不符均失败。完整审计文件是排查证据，不由网页重复下载。

页面启动与外置运行数据使用 schema `7`，完整架构数据包使用 schema `6`，Mermaid 几何输入版本见[渲染身份](../../tools/ecs-diagrams/src/mermaid-cache.mjs)。完整检查从原模型独立推导可见规范关系、函数卡片成员和显示边分组，核对每条可见关系恰好属于一个显示组以及两侧成员方向。赋值 `modify` 保留在原模型审计，网页展示范围与[显示规则](README.md#对象专属图)一致。混用旧运行数据、组成员或几何输入的产物须重新生成。

打包器从已通过完整校验的产物构建独立静态包，保留全部浏览与下载所需资源、源码快照以及本地 Graphviz／Mermaid 运行库，排除主模型、调用图和编译事实；删减审计文件只能由打包器完成，不能直接修改完整审计清单。完整 SVG 下载使用静态原生产物，查看器的[连线流动](README.md#节点与连线视觉规范)由页面独立维护。发布清单独立描述静态包的实际文件与 SHA-256，不能沿用要求审计文件的完整清单再手工删文件。校验同时检查额外文件、资源闭包、相对路径、构建来源及体积预算；静态包上限为 `900000000` 字节，超出即失败。

静态包根入口为 `index.html`，架构页和源码页位于 `pages/`；项目 Pages 子路径下全部资源使用相对 URL。[本地预览服务](README.md#本地自动预览)默认读取调用方的完整审计输出，可显式指定实际静态包；目录参数按仓库根解析，直接双击 HTML 不能替代 HTTP 资源加载。

```bash
npm run serve --prefix tools/ecs-diagrams -- --directory tmp/ecs-architecture/pages
```
