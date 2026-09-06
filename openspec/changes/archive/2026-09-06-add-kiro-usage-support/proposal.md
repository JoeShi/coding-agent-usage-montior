## Why

用户在 Kiro CLI（AWS 的 agentic 编程工具）上也有订阅套餐，其额度以 Credits 计量、按月重置，目前只能在 Kiro CLI 内用 `/usage` 查看。应用已支持 Kimi Code 和火山 AgentPlan，增加 Kiro 后可以在状态栏一处看全三个 agent 订阅的用量。

## What Changes

- 新增 Kiro 数据源：调用 CodeWhisperer 管控面 `GetUsageLimits` 接口（`POST https://codewhisperer.us-east-1.amazonaws.com/`，与 Kiro CLI `/usage` 同源），使用用户在设置窗口配置的 Kiro API Key（`ksk_...`）以 Bearer 方式鉴权，无需签名、无需刷新
- 解析订阅档位（subscriptionTitle）、月度 Credits 已用/配额/重置日期、超额计费状态（overage 开关、超额 credits、预估费用）与账号邮箱
- 设置窗口新增 Kiro API Key 的配置项，存 macOS Keychain（与火山 AK/SK 同构），保存时通过一次真实调用验证
- 状态栏压缩显示增加第三个数据源标签 `R`（如 `K:17% A:45% R:12%`），配置新增 `show_kiro` 开关

## Capabilities

### New Capabilities

- `kiro-usage`: 通过 Kiro API Key 查询订阅套餐的月度 Credits 用量与超额计费状态

### Modified Capabilities

- `credential-config`: 新增 Kiro API Key 的配置、验证、Keychain 存储与清除
- `menubar-display`: 状态栏压缩格式扩展为三个数据源，新增 `R` 标签

## Impact

- 代码：`src-tauri/src/providers/kiro.rs`（新增）、`model.rs`（DataSource 加 `KiroCli`）、`credentials.rs`（Keychain 加 `kiro_api_key`）、`lib.rs`（刷新聚合、托盘标题、IPC 命令）、`config.rs`（`show_kiro`）、前端设置页与卡片
- 外部依赖：`codewhisperer.us-east-1.amazonaws.com`（唯一可用 region）
- 已知风险：该接口校验 User-Agent，需伪装为 `aws-sdk-rust ... app/AmazonQ-For-CLI`，AWS 收紧 UA 白名单会导致该数据源失效
