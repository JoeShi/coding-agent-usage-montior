## Why

当前应用只能监控 Kimi Code、火山方舟 AgentPlan 与 Kiro，使用 ChatGPT 套餐登录 Codex CLI 的用户无法在同一状态栏应用中查看 Codex 的滚动配额。Codex 已通过 app-server 提供结构化、只读的账号限额接口，可以在不接管用户凭证的前提下补齐这一数据源。

## What Changes

- 新增 Codex 用量数据源，通过本机 `codex app-server --stdio` 的 JSON-RPC 接口读取 ChatGPT 套餐限额。
- 展示默认 Codex 限额桶及服务端返回的其他限额桶，包括主/次滚动窗口的已用百分比、窗口时长和重置时间。
- 展示服务端提供的 ChatGPT 套餐类型，并让 Codex 窗口参与现有 80% 告警、加速轮询和过期数据保留行为。
- 区分 Codex CLI 未安装、未登录、仅使用 API Key、需要重新登录、鉴权失败和临时读取失败等状态，并提供可执行的提示。
- 复用 Codex 自己的文件或系统钥匙串凭证及自动刷新能力；应用不读取、复制或持久化 `auth.json` 中的令牌。
- 不包含 OpenAI Platform API Key 账单/用量、Codex token 活跃度历史或 earned reset credit 消费操作。

## Capabilities

### New Capabilities

- `codex-usage`: 通过 Codex app-server 安全读取并规范化 ChatGPT 套餐限额、套餐信息与数据源状态。

### Modified Capabilities

无。现有 `usage-polling` 与 `menubar-display` 已以“所有数据源/任一窗口”定义通用轮询、告警和明细行为；Codex 作为新数据源遵循这些既有要求。

## Impact

- Rust：新增 `src-tauri/src/providers/codex.rs`，并修改 provider 注册、数据源枚举、并发刷新与相关验证。
- 前端：在 `src/App.tsx` 手工镜像 Codex 数据源类型和中文显示名称；现有通用窗口卡片继续负责渲染。
- 本机集成：运行时依赖可发现且支持 app-server 账号接口的 Codex CLI；不新增应用自管凭证或网络 API 密钥。
- 文档与验证：更新 README，并为 `src-tauri/examples/probe.rs` 增加显式的非 CI 实时探测入口。
- 依赖：预计复用现有 Rust 依赖，无需新增 crate。
