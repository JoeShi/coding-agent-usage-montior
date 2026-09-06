# Design: 增加 Kiro 数据源

## Context

应用已有两个数据源的完整骨架（见已归档 change `2026-09-06-add-agent-usage-menubar-app` 的 design.md）：provider 输出统一的 `UsageSnapshot`，调度器聚合刷新，托盘显示最紧张窗口。Kiro 查询方式已从参考实现（coding-agent-benchmark 的 `scripts/kiro_account_usage.py`）完整验证过，无未知协议风险。

## Goals / Non-Goals

**Goals**

- Kiro 成为与 Kimi/Ark 平级的第三个数据源，接入现有轮询、托盘、明细视图

**Non-Goals**

- 不支持多把 Kiro key 轮换（监控场景单 key 足够）
- 不复用本机 kiro-cli 的 IdC 登录态（本机是公司 IdC 身份，与个人订阅 API key 是两套账号体系；且 IdC token 15 分钟过期，复用价值低）
- 不展示 Kiro 的 per-model 明细（GetUsageLimits 无此维度）

## Decisions

### D1: 凭证只有 GUI 配置一条路

Kiro API key 不落盘于 CLI 侧（`KIRO_API_KEY` 是环境变量），无法像 Kimi 那样共享 CLI 凭证。设置窗口输入 → 保存时真实调用验证 → 存 Keychain（`kiro_api_key`），完全复用火山 AK/SK 的既有模式。验证调用即端到端测试，不需要额外的凭证准备步骤。

### D2: 复用 reqwest 直连，无签名逻辑

`GetUsageLimits` 是纯 Bearer + 固定 header 的 JSON-over-HTTP POST，不碰 volc_sigv4。唯一特殊处理：UA 伪装（见风险）。

### D3: 快照映射

Kiro 只有一个配额窗口 → 单个 `QuotaWindow{label: "monthly"}`；`SourceExtras` 增加 kiro 相关字段（订阅档位、overage 开关/超额 credits/预估费用、邮箱）。为避免 extras 字段无限膨胀，将现有 `SourceExtras` 加上 `plan_title` / `overage_enabled` / `overage_credits` / `overage_cost_usd` / `account_email` 可选字段（`plan_tier` 与 `plan_title` 语义重叠，保留 `plan_tier` 给 Ark，Kiro 用 `plan_title`——或统一改名，实现时选其一并同步前端）。

### D4: 托盘第三个标签用 `R`

`K`=Kimi、`A`=Ark 已占用，Kiro 取 `R`（KiRo）。三标签全量约 `K:17% A:45% R:12%`（~18 字符），仍在状态栏安全宽度内。`config.show_kiro` 控制显隐。

## Risks / Trade-offs

- [AWS 收紧 UA 白名单导致 403] → spec 已将该行为固化为可检测的鉴权错误状态；失效时界面明确提示，不影响其他数据源；修复只需更新 UA 常量
- [us-east-1 是唯一 codewhisperer 端点，区域硬编码] → 参考实现已验证其他 region 无法解析；若 AWS 后续开放多 region，改动是一个常量
- [超额费用字段类型不稳定（数字/缺失）] → 解析沿用防御式风格，缺失即不显示

## Migration Plan

无数据迁移。`config.toml` 新增 `show_kiro` 字段带默认值，旧配置文件向后兼容（serde default）。

## Open Questions

（无）
