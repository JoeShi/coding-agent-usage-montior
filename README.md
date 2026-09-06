# Agent Plan Monitor

macOS 状态栏应用，聚合监控 AI agent 订阅的额度使用情况。

支持的数据源：

- **Kimi Code** — 自动复用本机 Kimi Code CLI 的 OAuth 凭证（`~/.kimi-code/credentials/kimi-code.json`），token 过期自动刷新并原子写回。展示周配额、5 小时滚动窗口、会员等级、Extra Usage 余额
- **火山引擎方舟 AgentPlan** — 管控面 `GetAFPUsage`（5h/1d/1w/1m 四个滚动窗口 AFP 配额）+ `GetUsageDetails`（分模型明细）。凭证两条路：设置窗口配置长效 AK/SK（存 macOS Keychain，建议 IAM 子用户 + `ArkReadOnlyAccess`），或自动复用 arkcli 的 SSO 登录态（零配置）

特性：默认 5 分钟轮询、点击状态栏立即刷新、任一窗口用量 ≥80% 时自动加速轮询并红色告警。

## 开发

```bash
npm install
npm run tauri dev        # 开发模式
(cd src-tauri && cargo test)   # 后端测试
cargo run --example probe --manifest-path src-tauri/Cargo.toml  # 真实链路探针
```

## 构建

```bash
npm run tauri build
# 产物: src-tauri/target/release/bundle/macos/agent-plan-monitor.app
```

## 设计文档

见 `openspec/changes/add-agent-usage-menubar-app/`（proposal / design / specs / tasks）。
