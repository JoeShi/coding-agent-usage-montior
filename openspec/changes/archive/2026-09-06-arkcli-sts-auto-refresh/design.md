# Design: arkcli-sts-auto-refresh

## Context

动机见 proposal.md。当前解析链位于 `src-tauri/src/credentials.rs`：`resolve_ark_credentials()` → Keychain AK/SK 优先，fallback `arkcli_sts()` = `arkcli_sts_from_identities()` → `arkcli_sts_from_env()`。sts.json 过期（`credentials.rs:149`）即返回 `ArkCliExpired`，无任何续期尝试。三个调用点：`providers/ark.rs` 的 `fetch_afp_usage`（:182）与 `fetch_usage_details`（:240）（均为 async），以及 `lib.rs:222` 的同步 Tauri command `get_ark_cred_status`。

实测依据（已验证，直接采信）：arkcli 在 SSO identity 有效时执行任意命令即静默续期 STS 并回写 sts.json（status 输出 `reason: identity_sts_refreshed`）；`~/.arkcli/.env` 只在 login 时写入，续期不更新。

## Goals / Non-Goals

**Goals:**

- sts.json 过期时自动触发 arkcli 静默续期，用户在 identity 有效期内永远不需要手动 `arkcli auth login`。
- 续期对调用方透明：解析链签名不变（`resolve_ark_credentials()` 仍返回同样的 `Result<ArkCredentials, ArkCredError>`），错误语义不变。
- 子进程受控：不阻塞 async runtime、不 spawn 风暴、找不到 arkcli 时优雅降级。

**Non-Goals:**

- 不自行实现 SSO/STS 刷新协议（不解析 identity token、不调火山 STS 接口）——续期完全委托 arkcli 二进制。
- 不改动 Kimi Code / Kiro 数据源的任何逻辑。
- 不持久化 AK/SK，不引入新依赖。

## Decisions

### D1: 续期触发点放在解析链内部，而非轮询器或定时任务

`arkcli_sts()` 内：identities 路径读不到新鲜凭证时，就地尝试一次续期再重读。所有调用点（fetch、设置窗口状态查询）自动受益，无需各自改造。

- 备选：起一个后台定时任务每 N 分钟跑 `arkcli auth status`。否决——凭空多一个生命周期要管理，且应用在凭证新鲜时会做无用功；惰性触发（用到才续）天然与请求频率对齐。

### D2: 触发条件 = "identities 目录存在 且 没有可用 sts.json"

覆盖过期、60 秒内将过期、sts.json 缺失/损坏三种情况，统一为"没有可用凭证就问 arkcli 要"。目录不存在（纯 Keychain 用户、未装 arkcli）则直接跳过，不起子进程。

### D3: 移除 .env 回退路径

.env 只在 login 时写入且续期不更新，其有效窗口（login 后 15 分钟内）恒被 identities/sts.json 覆盖，是冗余死路径。移除后错误语义变干净：`NotConfigured` = identities 目录不存在（没登录过）；`ArkCliExpired` = 登录过但 identity 真失效。同时删除 `arkcli_sts_from_env()` 及其测试 `env_parse_skips_empty_values`。

### D4: 子进程控制参数与实现

- **30s 超时**：`std::process::Command` 无内建超时，用 `try_wait` 轮询（100ms 间隔）+ 超时 `kill()`，不引入新 crate。
- **90s 冷却 + Mutex 串行化**：全局 `static REFRESH_LOCK: OnceLock<Mutex<RefreshState>>`，`RefreshState { last_attempt: Option<Instant> }`。冷却期内直接跳过续期。轮询加速到 30s 时也不会反复 spawn。
- **二进制查找顺序**：`which arkcli`（继承 PATH）→ `~/.nvm/versions/node/*/bin/arkcli`（glob 取字典序最后一个，通常是最新版本；GUI app 从 Finder 启动时 PATH 极简，此路径是主路径）→ `npm prefix -g` + `/bin/arkcli` → `/usr/local/bin/arkcli`、`/opt/homebrew/bin/arkcli`。全部落空则静默跳过续期（不报错，按无可用凭证处理）。
- 续期命令：`arkcli auth status --format json`，成功判定 = exit code 0 后重读 sts.json 拿到新鲜凭证。stdout 内容不解析（只要它回写了 sts.json 就够）。

### D5: 60s 安全余量抽为纯函数

`fn is_sts_expired(expires_ms: i64, now_ms: i64) -> bool { expires_ms <= now_ms + 60_000 }`，identities 路径使用，可单测。这是本 change 唯一新增的单元测试面；子进程查找与 spawn 靠手动验收（模拟过期 / 改名二进制），不做依赖注入。

### D6: async 隔离

`fetch_afp_usage` / `fetch_usage_details` 中的 `credentials::resolve_ark_credentials()` 调用包 `tokio::task::spawn_blocking`（解析现在最长可能阻塞 30s）。`get_ark_cred_status` command 改 `async` + 内部 `spawn_blocking`，避免设置窗口打开时赶上续期阻塞 IPC。

## Risks / Trade-offs

- [arkcli 版本行为差异：旧版 arkcli 未必有静默续期] → 续期失败或重读仍过期时回退到原有 `ArkCliExpired` 提示路径，行为不劣于现状。
- [续期进程悬挂（如 arkcli 等待网络）] → 30s 硬超时 kill；最坏情况是本次 fetch 延迟 30s 后报 NeedRelogin，90s 冷却保证不会连续发生。
- [并发 fetch（配额 + 明细）同时触发续期] → Mutex 串行化，第二个调用看到冷却期内的 `last_attempt` 直接跳过，最多一个 arkcli 进程。
- [移除 .env 路径影响"只配了 .env 没登过 arkcli"的用户] → 该路径的凭证本就活不过 login 后 15 分钟，实际无人能依赖它；此类用户会看到 NotConfigured，引导其 login 或配置 AK/SK，语义更正确。
- [`get_ark_cred_status` 改 async 的 IPC 语义] → 前端 `invoke` 调用方式不变（Tauri 自动 await Promise），无前端改动。
