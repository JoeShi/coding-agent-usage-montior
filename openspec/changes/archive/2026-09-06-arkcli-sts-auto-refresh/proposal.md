# Proposal: arkcli-sts-auto-refresh

## Why

arkcli 的 STS 临时凭证有效期只有约 15 分钟。本应用只被动读取 `~/.arkcli/identities/<identity>/sts.json`，从不调用 arkcli，因此凭证一旦过期就报"需要重新登录"，用户被迫频繁手动运行 `arkcli auth login`。实测验证：arkcli 在 SSO identity 仍有效时会静默自动续期 STS——执行 `arkcli auth status --format json`（无需浏览器、无需交互）即可刷新 sts.json。应用缺的不是用户重新登录，而是主动触发一次 arkcli 的静默续期。

## What Changes

- 在 arkcli 凭证解析链中加入静默续期：sts.json 已过期（或 60 秒内将过期）时，先运行 `arkcli auth status --format json` 触发 arkcli 自动续期，成功后重读 sts.json；仅当续期失败（identity 真正失效）才提示用户手动 `arkcli auth login`。
- **BREAKING**（内部行为）：移除 `~/.arkcli/.env`（VOLCENGINE_STS_*）回退路径。该文件只在 login 时写入、静默续期不更新，永远是过期死值；identities/sts.json 在自愈后已完全覆盖其作用。identities 目录不存在时直接判为未配置。
- 过期判定统一加 60 秒安全余量，避免签名请求用到即将过期的 token。
- 子进程保护：30 秒超时（超时 kill）、90 秒冷却防抖、Mutex 串行化防并发重复 spawn；arkcli 二进制查找带 PATH 兜底（which → nvm → npm prefix -g → /usr/local/bin → /opt/homebrew/bin），找不到则静默跳过续期。
- `fetch_afp_usage` / `fetch_usage_details` 中的同步凭证解析包入 `tokio::task::spawn_blocking`，避免最长 30s 的子进程阻塞 async runtime；`get_ark_cred_status` command 改为 async。
- 前端过期提示文案更新为"自动续期失败"语义。

## Capabilities

### New Capabilities

（无）

### Modified Capabilities

- `ark-agent-plan-usage`: 修改"arkcli 登录态 fallback"需求——从"MUST NOT 自行刷新 STS、过期即提示重新登录"反转为"过期时主动触发 arkcli 静默续期，仅续期失败才提示重新登录"；并明确凭证来源收敛为 identities/sts.json 单一路径（移除 .env 回退）。

## Impact

- `src-tauri/src/credentials.rs`：解析链改造、续期子进程、60s 余量、移除 .env 路径及对应测试。
- `src-tauri/src/providers/ark.rs`：两处凭证解析调用包 `spawn_blocking`。
- `src-tauri/src/lib.rs`：`get_ark_cred_status` 改 async。
- `src/App.tsx`：NeedRelogin 文案。
- 不引入新依赖，不新增持久化凭证（符合"优先 SSO + 临时 STS，不鼓励持久化 AK/SK"的约束）。
