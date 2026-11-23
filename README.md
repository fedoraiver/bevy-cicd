# Bevy CI/CD

供 Bevy 项目复用的 GitHub Actions 工作流，覆盖 Rust 检查、Bevy 编译器规则、ECS 架构图生成和 GitHub Pages 发布。

- [工作流接入与配置示例](docs/workflows.md)
- [仓库维护概览](docs/README.md)
- [ECS 架构图参考](docs/ecs-diagrams/README.md)

## ECS 调度导出兼容性

共享调度启动器要求选中的 Cargo 包提供库目标，并且其依赖图中只有一个名为 `bevy` 的包，来源为 crates.io，版本为 `0.20.x`。在库根提供公开的无参数 `build_app` 并在 Cargo 清单中声明它。已有 `debugdump` 入口的项目可继续使用旧导出方式。[配置示例](docs/workflows.md#生成并发布-ecs-架构图)
