# Agent Note: 共享 Bevy CI/CD

Status: implemented — 共享规则与发布入口已实现

## Problem

demo 的质量检查和架构图发布依赖项目内路径。其他 Bevy 项目需要复用规则，并独立管理 Cargo 配置和架构图生成。

## Decision

集中维护三个工作流。常规 Rust 检查读取调用方的工具链，编译器规则使用共享仓库固定的 nightly。Pages 接收调用方已验证的静态站点及同一次运行的证明。

## Changes

[接入指南](../../../README.md) 说明工作流输入、版本及产物契约。分析器保留源规则实现和回归，并带有独立的 Bevy 依赖锁。部署验证默认分支的实时 HEAD，并比较上线的证明和首页哈希。

## Alternatives considered

在每个调用仓库复制工作流会产生多份规则。固定版本的共享工作流允许调用方独立安排升级。把图生成留在项目内，使同一发布流程可以服务不同的静态生成方式。

## Consequences

编译器规则依赖固定的 Rust ABI 和已验证的 Bevy 版本。Pages 局部重跑依赖构建产物保留期。

## Verification

共享仓库回归覆盖原编译器规则、正反例和 Pages 发布证明。bevy-destruction 的接入 PR 记录跨仓库 GitHub Actions 和线上验证结果。
