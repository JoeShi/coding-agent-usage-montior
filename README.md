# Agent Plan Monitor

macOS 状态栏应用，聚合监控 AI agent 订阅套餐的额度使用情况。

支持的数据源：

- **Kimi Code** — 自动复用本机 Kimi Code CLI 的 OAuth 凭证（`~/.kimi-code/credentials/kimi-code.json`），token 过期时由应用按 Kimi 协议刷新。展示周配额、5 小时滚动窗口、会员等级和 Extra Usage 余额
- **火山引擎方舟 AgentPlan** — 自动复用 arkcli SSO 的短期 STS 凭证（`~/.arkcli/identities/<id>/sts.json`），过期时通过 arkcli 静默刷新。展示 5h/1d/1w/1m AFP 配额和分模型明细；应用不读取 `~/.arkcli/.env`，也不保存 AK/SK
- **Kiro** — 使用 `ksk_` API Key 查询月度 Credits、套餐和超额计费信息；API Key 仅存储在 macOS Keychain
- **Codex** — 通过本机 `codex app-server --stdio` 复用 ChatGPT 登录，展示服务端返回的 5 小时、每周及其他限额桶。应用不读取或复制 `~/.codex/auth.json`；仅使用 OpenAI Platform API Key 登录时无法查询 ChatGPT 套餐限额

特性：默认 5 分钟轮询、点击状态栏立即刷新、任一窗口用量 ≥80% 时自动加速轮询并显示红色告警。网络或临时服务失败时保留上一次成功窗口并标记为数据过期。

## Codex 前提与排障

Codex 数据源需要：

1. 本机已安装支持 app-server 账号限额接口的 Codex CLI；
2. 使用 ChatGPT 账号登录，而不是仅配置 OpenAI Platform API Key。

```bash
codex login status
codex login             # 未登录或登录失效时重新登录
codex update            # 提示接口不受支持时升级
```

凭证仍由 Codex 自己保存在其配置的文件、系统钥匙串或其他 credential store 中；Agent Plan Monitor 只与本机 Codex 子进程通信，不记录完整协议响应或账号令牌。

## 开发

```bash
npm install
npm run tauri dev
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

真实链路探针会使用本机 Kimi OAuth、arkcli SSO、Codex ChatGPT 登录、Kiro Keychain，以及可选的 `VOLCENGINE_ACCESS_KEY` / `VOLCENGINE_SECRET_KEY` 环境变量。它会访问真实服务，**不能作为 CI 检查**：

```bash
cargo run --example probe --manifest-path src-tauri/Cargo.toml
```

## 构建

```bash
npm run tauri build
# 产物: src-tauri/target/release/bundle/macos/agent-plan-monitor.app
```

## 设计文档

当前 Codex 变更见 `openspec/changes/add-codex-cli-support/`；已归档的历史设计位于 `openspec/changes/archive/`。
