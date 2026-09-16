## Context

参见 `proposal.md` 的动机。当前后端以 `UsageSnapshot` 统一 Kimi、Ark、Kiro 三个 provider，`refresh_all` 并发获取后通过 `merge_snapshot` 保留临时失败前的成功窗口；前端在 `src/App.tsx` 手工镜像 Rust serde 类型并通用渲染窗口。

Codex CLI 可能把登录凭证放在 `$CODEX_HOME/auth.json` 或操作系统凭证存储中，并负责 ChatGPT token 自动刷新。官方 app-server 通过 stdio 上的逐行 JSON-RPC 暴露 `account/read` 与 `account/rateLimits/read`，后者返回默认单桶视图、可选多桶视图、主/次窗口、套餐和 credits 元数据。API-key-only 登录不提供 ChatGPT 套餐限额。macOS GUI 进程的 PATH 通常不包含通过 nvm 等 Node 版本管理器安装的 `codex`。

## Goals / Non-Goals

**Goals:**

- 在不接触 Codex token 的情况下，把 ChatGPT 套餐限额转换为现有 `UsageSnapshot`。
- 对多限额桶、可选窗口、旧版单桶响应和 app-server 能力缺失进行确定性处理。
- 保持 provider 无状态、轮询无重叠、失败可超时，并复用现有状态、告警和 stale merge 行为。
- 让已安装应用能够在 GUI PATH 不完整时发现用户实际使用的 Codex CLI。

**Non-Goals:**

- 读取或修改 `auth.json`，或由本应用刷新/保存 OpenAI 凭证。
- 查询 OpenAI Platform API Key 的账单、余额或 API 速率限制。
- 展示 token 活跃度历史，或消费 earned rate-limit reset credits。
- 在本次变更中维护常驻 Codex daemon、WebSocket 连接或独立轮询周期。

## Decisions

### 1. 通过临时 stdio app-server 读取限额

每次 Codex provider 刷新时启动 `codex app-server --stdio`，完成 `initialize` / `initialized` 握手，然后依次调用 `account/read` 和 `account/rateLimits/read`。客户端按 JSON-RPC `id` 关联响应并忽略无关通知；拿到结果或错误后终止子进程。整个会话放入 blocking worker，设置总超时并在超时后杀死子进程，避免阻塞 Tokio worker 或遗留进程。

选择临时进程是因为它符合现有无状态 provider 接口，不要求应用接管 daemon 生命周期；最低轮询间隔也限制了启动频率。如果实测启动成本不可接受，再以独立变更引入持久客户端和重连状态机。

未选择直接读取 `auth.json` 并请求 ChatGPT backend：该方案不支持 keyring/ephemeral credential store，绑定私有 token 与 endpoint 格式，并迫使本应用承担 token 刷新和敏感日志风险。未选择解析 TUI 输出，因为它不是稳定的机器接口。未选择 WebSocket，因为本机单请求无需监听端口或引入额外认证面。

### 2. 按能力检测而非硬编码最低版本

系统先发现并启动 `codex`，再根据 app-server 子命令、初始化和 `account/rateLimits/read` 的实际响应判断能力。找不到命令或 RPC method 时返回带安装/升级提示的 `NotConfigured`，而不是比较可能受预发布、回移植或发行渠道影响的版本字符串。

Codex 二进制发现沿用 arkcli 的 GUI 安全策略：先检查当前 PATH，再检查登录 shell 和常见 Node 版本管理器安装位置；诊断只记录最终路径类别和错误，不记录环境中的 secret。实现先保留 Codex 私有 resolver，避免为了一个新调用点重构 Ark 凭证模块；确认出现第三个相同调用点后再提取通用 CLI discovery 模块。

### 3. 将协议与业务映射封装在 Codex provider 内

`src-tauri/src/providers/codex.rs` 对外只暴露获取 `UsageSnapshot` 的入口，内部封装：

- app-server 请求/响应 serde 类型；
- 有上限的逐行协议读写、超时和子进程清理；
- 账号模式与 RPC 错误到 `SourceStatus` 的映射；
- rate-limit bucket 到 `QuotaWindow` 的转换。

调用顺序为：

```text
refresh_all
    |
    +--> codex::fetch_usage
            |
            +--> discover codex
            +--> spawn app-server --stdio
            +--> initialize / initialized
            +--> account/read
            +--> account/rateLimits/read
            +--> normalize buckets
            +--> UsageSnapshot
```

协议 stdout 视为不可信输入：限制单行和累计响应大小，只解析预期字段；stderr 仅保留截断且脱敏后的诊断摘要。日志不得包含 stdin/stdout 原文、Authorization 内容或账号响应。

### 4. 多桶优先，单桶回退，并生成稳定窗口

新增 `DataSource::Codex`，serde 值为 `codex`。当 `rateLimitsByLimitId` 非空时只使用该多桶视图；否则使用 `rateLimits` 单桶视图，避免默认桶重复。桶按默认 `codex` 优先、其余 `limitId` 字典序排列；每个桶固定先 primary 后 secondary，确保刷新间 UI 顺序稳定。

每个窗口映射为：

- `used = max(usedPercent, 0)`；
- `quota = 100`；
- `resets_at = resetsAt` 的 Unix 秒转换结果；
- 名称由 `limitName`（若存在）或 `limitId` 与窗口时长组合。

常见 300 分钟和 10080 分钟分别显示为“5 小时”和“每周”；其他值按分钟、小时或天生成不丢精度的标签。缺失窗口直接省略，不生成 `0/100`。套餐类型写入现有 `SourceExtras.plan_tier`；credits 与 reset-credit 元数据本次解析时允许忽略，不扩展前端模型。

### 5. 复用现有状态和刷新语义

状态映射如下：

- 找不到 CLI、未登录、API-key-only、缺少 RPC 能力：`NotConfigured`，附安装、ChatGPT 登录或升级提示；
- Codex 报告登录不可恢复：`NeedRelogin`；
- 账号/工作区权限拒绝：`AuthError`；
- 启动失败、超时、截断、JSON/RPC 异常、临时服务错误或成功响应无有效窗口：`Stale`；
- 至少一个有效窗口：`Ok`。

`refresh_all` 增加第四个并发 future，并继续通过 `merge_snapshot` 合并。Codex 不创建自己的 timer；其 `Ok` 窗口自动参与 `over_threshold`。本变更不改变现有 merge 或 80% 阈值语义。

### 6. 前端只扩展数据源身份

`src/App.tsx` 将手工镜像的 source union 增加 `"codex"`，并在 `SOURCE_NAME` 映射为“Codex”。套餐使用现有 `plan_tier`，所有限额窗口由通用 `SourceCard` / `WindowRow` 渲染，因此不增加 Codex 专属设置或卡片分支。未配置状态中的错误提示负责告诉用户安装、升级或执行 ChatGPT 登录。

## Risks / Trade-offs

- [app-server 命令仍标记为 experimental，协议未来可能变化] → 使用 serde 可选字段、能力检测、单桶回退和清晰升级提示；不依赖私有 backend。
- [每次轮询启动 Codex 进程有延迟和资源成本] → 使用 bounded timeout，保持最小 30 秒轮询约束并在真实 probe 中测量；只有数据证明需要时才引入常驻进程。
- [GUI PATH 找不到 nvm 安装的 Codex] → 复用现有 arkcli 的登录 shell和常见安装目录发现策略，并提供不含凭证的路径诊断。
- [子进程输出异常导致内存增长或泄漏信息] → 限制行长/总量、只反序列化预期字段、截断脱敏 stderr，禁止记录完整协议消息。
- [新增未配置 Codex 卡片增加不使用 Codex 用户的视觉噪音] → 与当前所有 provider 均参与轮询和展示的行为保持一致；数据源显隐属于单独的产品决策。

## Migration Plan

1. 增加 Codex provider 与 `DataSource::Codex`，接入现有并发刷新和前端 source 映射。
2. 更新 README 的支持矩阵、ChatGPT 登录前提和隐私说明；在 `probe` 中增加显式 Codex 实时探测。
3. 运行 Rust 测试和前端构建，并由已使用 ChatGPT 登录的 Codex CLI 执行一次非 CI 实时 probe，确认多窗口、重置时间和套餐显示。
4. 发布无需配置迁移；未安装或未使用 ChatGPT 登录的用户只会看到可诊断的未配置状态。

回滚时移除 Codex provider 注册、枚举与前端 source 项即可；本变更不写入用户凭证或配置，因此没有数据回滚步骤。
