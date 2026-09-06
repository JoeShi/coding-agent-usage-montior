# Tasks: arkcli-sts-auto-refresh

## 1. credentials.rs 解析链改造

- [x] 1.1 新增纯函数 `is_sts_expired(expires_ms: i64, now_ms: i64) -> bool`（`expires_ms <= now_ms + 60_000`），替换 identities 路径的过期判定；为其添加单元测试（新鲜/刚好过期/余量内临期三种情况均覆盖），`cargo test` 通过
- [x] 1.2 实现 arkcli 二进制查找 `find_arkcli_binary()`：`which arkcli` → `~/.nvm/versions/node/*/bin/arkcli`（glob 取最新版本目录）→ `npm prefix -g` + `/bin/arkcli` → `/usr/local/bin/arkcli` → `/opt/homebrew/bin/arkcli`，全部落空返回 None；`cargo check` 无新增警告
- [x] 1.3 实现受控续期 `try_refresh_arkcli_sts()`：全局 `OnceLock<Mutex<RefreshState>>` 串行化 + 90s 冷却（冷却期内直接返回），`std::process::Command` 运行 `arkcli auth status --format json`，`try_wait` 轮询实现 30s 超时并 `kill()`；`~/.arkcli/identities/` 目录不存在或找不到二进制时静默跳过；`cargo check` 无新增警告
- [x] 1.4 改造 `arkcli_sts()`：identities 路径无可用凭证（过期/临期/缺失/损坏）且 identities 目录存在时调用 1.3 续期，随后重读一次 identities sts.json；仍无可用凭证才返回 `ArkCliExpired`
- [x] 1.5 删除 `arkcli_sts_from_env()` 及其测试 `env_parse_skips_empty_values`，确认全仓无 `VOLCENGINE_STS` 引用残留；`cargo test` 全部通过

## 2. 异步调用点隔离

- [x] 2.1 `providers/ark.rs` 的 `fetch_afp_usage` 与 `fetch_usage_details` 中 `credentials::resolve_ark_credentials()` 调用包入 `tokio::task::spawn_blocking`；`cargo test` 中 ark 相关测试全部通过
- [x] 2.2 `lib.rs` 的 `get_ark_cred_status` command 改为 `async fn`，内部解析包 `spawn_blocking`；确认前端 `invoke("get_ark_cred_status")` 调用点无需改动；`cargo check` 无新增警告

## 3. 前端文案

- [x] 3.1 `src/App.tsx:254` 附近 NeedRelogin 提示改为"arkcli 登录态失效（自动续期失败）— 请运行 arkcli auth login 或配置 AK/SK"；`npm run build`（或 tsc）通过

## 4. 验证

- [x] 4.1 `cd src-tauri && cargo test` 全部通过且 `cargo check` 无新增警告
- [x] 4.2 手动验证自动续期：将 `~/.arkcli/identities/*/sts.json` 的 `expires_at` 改到过去，启动 app（或等一个轮询周期），确认自动续期成功、用量正常展示、不再提示重新登录
- [x] 4.3 手动验证优雅降级：临时将 arkcli 二进制改名后触发一次过期场景，确认无报错、行为回退为原有的 NeedRelogin 提示，随后恢复二进制名称
