## Why

开发者同时订阅了多个 AI agent 服务（火山引擎方舟 AgentPlan、Kimi Code），各家额度都是滚动窗口限流（5 小时 / 周 / 月），但查询入口分散在各自的 CLI 命令和网页控制台里。想知道"还剩多少额度、什么时候刷新"需要手动跑命令或开网页，经常在触发限流时才发现额度耗尽。

## What Changes

- 新建一个 Tauri macOS 状态栏应用，常驻菜单栏，聚合展示各 agent 订阅的额度使用情况
- Kimi Code 数据源：直接读取本机 `~/.kimi-code/credentials/kimi-code.json` 中的 OAuth 凭证，调用 `GET https://api.kimi.com/coding/v1/usages` 查询；access_token 过期时自动用 refresh_token 刷新并原子写回凭证文件
- 火山方舟 AgentPlan 数据源：GUI 配置火山 IAM AK/SK（存 macOS Keychain），以 V4 签名调用管控面接口 `GetAFPUsage`（5h/1d/1w/1m 四窗口配额）与 `GetUsageDetails`（分模型用量明细）；若检测到本机 arkcli 已有 SSO 登录态，可复用其 STS 凭证作为零配置 fallback
- 轮询调度：默认每 5 分钟刷新，点击状态栏图标强制刷新，任一窗口用量超过 80% 时自动加快轮询
- 状态栏压缩显示最紧张的指标（如 `K:90% A:45%`），下拉菜单展示完整明细，设置窗口配置 AK/SK 与轮询偏好

## Capabilities

### New Capabilities

- `kimi-code-usage`: 通过 Kimi Code CLI 共享的 OAuth 凭证查询订阅用量（周配额、5 小时滚动窗口、Extra Usage 余额），含 token 自动刷新与竞争写回处理
- `ark-agent-plan-usage`: 通过火山引擎管控面 API 查询方舟 AgentPlan 套餐用量（四窗口 AFP 配额、分模型明细），支持 AK/SK 配置与 arkcli 登录态 fallback
- `usage-polling`: 轮询调度器，支持可配置的基础间隔、手动强制刷新、以及接近限流时的自适应加速
- `menubar-display`: macOS 状态栏常驻显示与下拉明细视图，压缩格式展示最紧张的配额指标
- `credential-config`: 设置窗口，配置火山 AK/SK（Keychain 存储）、轮询频率与显示偏好

### Modified Capabilities

（无——本仓库尚无任何已有 spec）

## Impact

- 全新 Tauri 项目（Rust 后端 + Web 前端），当前仓库为空仓库，无既有代码受影响
- 外部依赖：Kimi Code CLI 的凭证文件（`~/.kimi-code/credentials/kimi-code.json`，共享读写）、火山引擎 OpenAPI（`ark.cn-beijing.volcengineapi.com`）、可选的本机 arkcli 安装（`~/.arkcli`）
- 关键 Rust 依赖：`tauri` v2、`keyring`（Keychain）、`reqwest`（HTTP）、火山 V4 签名（手写 HMAC-SHA256 链或社区 crate）
