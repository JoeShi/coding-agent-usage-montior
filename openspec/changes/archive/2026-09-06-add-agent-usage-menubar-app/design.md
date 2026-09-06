# Design: Agent 用量监控状态栏应用

## Context

空仓库，从零搭建。探索阶段已验证两条数据链路（见 proposal.md）：

- Kimi Code：`GET https://api.kimi.com/coding/v1/usages`，Bearer 为本机 CLI 的 OAuth token（15 分钟有效期），实测可用
- 火山 AgentPlan：`GetAFPUsage` / `GetUsageDetails` 管控面接口，必须 V4 签名；实测 Bearer API Key 被拒（`InvalidAuthorization`）；本机 arkcli 通过 SSO 派生 STS 签名可用

## Goals / Non-Goals

**Goals**

- 单一常驻 macOS 状态栏应用，聚合 Kimi Code 与火山 AgentPlan 的额度
- 凭证安全：敏感凭证只进 Keychain / 各 CLI 原生存储
- 资源占用低，适合常驻

**Non-Goals**

- 不支持 Windows / Linux
- 不支持火山企业版席位接口（GetSeatAFPUsage / GetSeatUsageDetails）
- 不做额度告警通知（仅视觉提示；系统通知留待后续）
- 不逆向火山 SSO 登录流程（不内嵌浏览器登录，不自刷 arkcli STS）

## Architecture

```
+-----------------------------------------------------------+
|  Tauri v2 App (macOS)                                     |
|                                                           |
|  Web 前端                   Rust 后端                      |
|  +------------------+      +---------------------------+  |
|  | 下拉明细视图      |      | tray (TrayIconBuilder)    |  |
|  | 设置窗口          |<---->| scheduler (tokio interval)|  |
|  +------------------+ IPC  | providers/                |  |
|                            |  ├─ kimi_code.rs           |  |
|                            |  └─ ark_agent_plan.rs      |  |
|                            | credentials/ (keyring)    |  |
|                            | volc_sigv4.rs             |  |
|                            +---------------------------+  |
+-----------------------------------------------------------+
        |                                |
        v                                v
 ~/.kimi-code/credentials/      ark.cn-beijing.volcengineapi.com
 api.kimi.com/coding/v1         (~/.arkcli STS fallback)
```

前端框架选定 **React + Vite**（团队熟悉度最高，Tauri 模板成熟）。

## Decisions

### D1: Tauri v2 + Rust 后端承担所有网络与凭证逻辑

前端只负责渲染；轮询、签名、token 刷新、Keychain 全在 Rust 侧。理由：凭证不出 Rust 进程边界，攻击面最小；火山 V4 签名用 Rust 手写（HMAC-SHA256 链，约 150 行，参考 `volcengine` 签名规范），不引入不成熟的三方 crate。

备选：Tauri 前端直连 API（`@tauri-apps/plugin-http`）——拒绝，Secret Key 会暴露给 WebView JS 上下文。

### D2: Kimi token 与 CLI 共享同一文件，原子写回

读 `~/.kimi-code/credentials/kimi-code.json`；过期时以 refresh_token 调 `POST https://auth.kimi.com/api/oauth/token`（form 表单，client_id 与 CLI 一致）。写回流程：写临时文件 → `rename` 原子替换；写回前重读并比对 refresh_token，若已被 CLI 轮换则以文件为准重试。`invalid_grant` 时重读文件重试一次，仍失败则标记"需要重新登录"。

备选：app 自持一份凭证独立刷新——拒绝，refresh_token 轮换会与 CLI 互相踢掉。

备选：只读不刷，依赖 CLI 保活——拒绝，CLI 不使用时 app 会在 15 分钟后失效。

### D3: 火山凭证双路：AK/SK 优先，arkcli STS fallback

- 主路径：用户在设置窗口填 AK/SK → 存 Keychain（`keyring` crate）→ 静态 V4 签名
- fallback：未配 AK/SK 时探测 `~/.arkcli/.env` 中的 `VOLCENGINE_STS_*`，未过期（`VOLCENGINE_STS_EXPIRES_AT_MS`）则带 `X-Security-Token` 头做 V4 签名；过期则提示重新登录，绝不自行刷新
- UI 明确展示当前凭证来源

备选：内嵌火山 SSO 登录——拒绝，逆向私有流程，成本高且易碎。

### D4: 轮询调度器（tokio）

`tokio::time::interval` 驱动；基础间隔可配（默认 5min）；手动刷新走独立 oneshot 触发不打乱 interval；自适应加速：任一窗口 used/quota ≥ 0.8 → 切换到 1min 间隔，回落后恢复；单次失败静默保留旧数据，下周期重试（无指数退避，间隔本身已够稀疏）。

### D5: 状态栏文本渲染

macOS 状态栏用 `TrayIconBuilder` 的 title 显示压缩文本（`K:90% A:45%`），超过 80% 时切换图标颜色（红/橙）。下拉菜单用原生 NSMenu（Tauri tray menu）做快速操作（刷新/设置/退出），完整明细开 WebView 窗口（popover 风格）。

备选：下拉直接全用 WebView popover——可作为后续演进，v1 用原生菜单更稳。

### D6: 数据模型

```rust
struct UsageSnapshot {
    source: DataSource,            // KimiCode | ArkAgentPlan
    status: SourceStatus,          // Ok | Stale | AuthError | NotConfigured | NeedRelogin
    windows: Vec<QuotaWindow>,     // label, used, quota, reset_at
    extras: SourceExtras,          // kimi: 会员等级/extra 余额; ark: 套餐档位
    fetched_at: DateTime,
}
```

所有 provider 输出统一为 `UsageSnapshot`，前端只消费该模型。非敏感配置（轮询间隔、显示偏好）存 `~/.config/agent-plan-monitor/config.toml`。

## Risks / Trade-offs

- [Kimi 凭证文件格式或端点随 CLI 升级变化] → 解析做容错（缺字段即标记错误而非 panic）；`/usages` 返回结构变更时在 UI 显示"数据解析失败"
- [refresh_token 轮换竞争仍可能偶发失败] → `invalid_grant` 重读重试；最终失败仅标记状态，不影响其他数据源
- [arkcli 的 STS 文件格式是私有实现，可能变动] → fallback 失败时优雅降级到"请配置 AK/SK"提示
- [火山 V4 签名手写实现出错的调试成本] → 先用 `GetAFPUsage` 做集成冒烟测试（该接口响应小、幂等），签名实现配单元测试（固定时间戳 + 已知 AK/SK 的签名向量）
- [状态栏文本过长被系统截断] → 压缩格式控制在 ~16 字符内，异常态用 `--` 占位

## Migration Plan

全新应用，无迁移。发布形态：`cargo tauri build` 产出 `.app` bundle，手动安装即可，暂不涉及签名公证与自动更新。

## Open Questions

（无——探索阶段已全部解决，答案见下文附录）

## Appendix: 探索阶段验证结果

- **Kimi `/usages` 负向形态**（源自 CLI 二进制解析器 `parseManagedUsagePayload`，与其实测行为一致）：所有顶层 section 均可空——无 Extra Usage 时 `boosterWallet` 缺失/为 null（或 `balance.type != "BOOSTER"`、`amount <= 0`）；未订阅时 `usage`/`limits` 缺失，应解析为 `{summary: null, limits: [], extraUsage: null}` 而非报错。金额字段（`amount`/`amountLeft`/`priceInCents`）为 1e-8 定点数（如 `7500000000` = ¥75.00）。
- **火山 `GetUsageDetails` 真实响应**（本机实测，status=200）：`Result.Details[]`，每条 `{BillingType: "WithinPlan"|…, ObjectName: <model-id>, Time: <epoch ms>, Unit: "Tokens", Usage: <number>}`，Time 按 QueryInterval 对齐。
