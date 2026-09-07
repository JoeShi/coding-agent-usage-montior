# Proposal: simplify-menubar-and-credentials

## Why

三处"删繁"：状态栏标题 `K:17% A:45% R:12%` 过长且扫读无结论，圆点图标的红/灰告警已足够；火山 AgentPlan 的 GetAFPUsage 返回的 AFPDaily 是幽灵字段（used 恒为 0 且小于 5h 用量，ResetTime 与 weekly 撞车），不是真实维度；arkcli 自动续期上线后火山数据源已锁定 arkcli 零配置路线，手动 AK/SK 配置入口成为死重。

## What Changes

- 状态栏标题改为纯圆点图标（红 = 任一窗口超阈值，灰 = 正常），不再显示各源百分比文本。**BREAKING**（设置项）：删除"状态栏显示"的 show_kimi / show_ark / show_kiro 开关及 config 对应字段（旧配置文件中的这些键反序列化时被忽略，无迁移问题）；三个数据源始终轮询并展示在明细面板。
- 火山 AgentPlan 配额窗口从四个（5h/daily/weekly/monthly）收敛为三个（5h/weekly/monthly），删除 AFPDaily 解析。**BREAKING**（内部行为）：移除手动 AK/SK 凭证路径——火山数据源仅以 arkcli SSO 登录态（自动续期 STS）为凭证来源；删除 Keychain AK/SK 读写、`ArkCredSource::AkSk`、凭证优先级逻辑及相关 Tauri command；设置窗口删除"火山引擎凭证"整个 section（含凭证来源状态展示）。

## Capabilities

### New Capabilities

（无）

### Modified Capabilities

- `menubar-display`: 状态栏标题从"各源百分比文本 + 圆点"改为纯圆点图标；删除按源显示开关。
- `ark-agent-plan-usage`: "AK/SK 签名查询套餐配额"需求改为三个窗口、凭证措辞从用户配置 AK/SK 改为 arkcli STS；"凭证优先级"需求 REMOVED（arkcli 成为唯一来源）。
- `credential-config`: "配置火山 AK/SK"需求 REMOVED；"凭证来源展示与清除"收敛为仅 Kiro。

## Impact

- `src-tauri/src/lib.rs`：删除 tray_title/source_tag 及测试、show_* 相关逻辑、save/clear/get_ark_cred_status 等 command。
- `src-tauri/src/config.rs`：删除 show_kimi/show_ark/show_kiro 字段。
- `src-tauri/src/credentials.rs`：删除 keychain_aksk/save_ark_aksk/clear_ark_aksk/ArkCredSource::AkSk/choose()，解析链收敛为 arkcli STS 单路径。
- `src-tauri/src/providers/ark.rs`：删除 AFPDaily 字段与 push、credential_source extra、测试 fixture 更新。
- `src/App.tsx`：删除"火山引擎凭证"section、"状态栏显示"section 及相关 state/handler。
- 不引入新依赖；Keychain 中遗留的旧 AK/SK 条目成为无害孤儿，不做清理。
