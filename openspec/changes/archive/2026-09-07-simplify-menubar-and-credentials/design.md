# Design: simplify-menubar-and-credentials

## Context

动机见 proposal.md。三个删除项互相独立但同属"删繁"，合并为一个 change。相关现状：托盘标题由 `lib.rs` 的 `tray_title()` 拼接、圆点图标由 `dot_icon(warn)` 生成；火山凭证解析链在 `credentials.rs`（上个 change 刚接入 arkcli 自动续期）；设置窗口在 `App.tsx`。

## Goals / Non-Goals

**Goals:**

- 状态栏只剩圆点图标，红/灰承担全部告警职责。
- 火山凭证栈收敛为 arkcli 单一路径，删掉手动 AK/SK 的全部代码与 UI。
- AFPDaily 幽灵字段不再出现在解析与展示中。

**Non-Goals:**

- 不清理 Keychain 中遗留的旧 AK/SK 条目（无害孤儿）。
- 不改变轮询调度、加速策略、明细面板内容与 popover 样式。
- 三个数据源始终轮询——不引入"按源禁用轮询"的新能力。

## Decisions

### D1: 托盘标题 = 空，图标承担一切

`tray_title()` 及 `source_tag()` 整体删除，`update_tray` 只设图标不设文字（`set_title(None)`）。告警逻辑 `over_threshold` 不变，继续驱动红点与轮询加速。

- 连带删除：config 的 `show_kimi/show_ark/show_kiro` 字段、App.tsx"状态栏显示"section、`tray_title_*` 三个单测。旧配置文件含 show_* 键——serde 默认忽略未知字段，无迁移代码。
- 备选：保留开关改为控制轮询/明细面板显示。否决——那是新能力而非简化，且单用户工具没有禁用某源的真实诉求。

### D2: 凭证栈收敛为 arkcli 单路径

`resolve_ark_credentials()` 简化为直接调用 arkcli 路径；删除 `choose()`、`keychain_aksk()`、`save_ark_aksk()`、`clear_ark_aksk()`、`ArkCredSource` 枚举（`credential_source` extra 一并删除——恒为 arkcli 无信息量）。lib.rs 删除 `get_ark_cred_status` / 保存 / 清除三个 command 及 AK/SK 验证查询逻辑；App.tsx 删除"火山引擎凭证"section 及其 state/handler。

- 删除后错误语义：`NotConfigured` = 没装/没登录 arkcli；`ArkCliExpired` = identity 失效（自动续期失败）。前端 NeedRelogin 文案改为只提示 `arkcli auth login`（不再提"或配置 AK/SK"）。
- 备选：保留 AK/SK 作为隐藏兜底。否决——死代码会继续收规格和维护的税，且违背"不持久化 AK/SK"的内部规范。

### D3: AFPDaily 从解析层删除

`AfpResult.daily` 字段与 `push("daily", ...)` 删除，测试 fixture 中的 AFPDaily 保留在 JSON 里以验证"服务端返回也正确忽略"——这正好回归 D3 的行为契约。

## Risks / Trade-offs

- [用户习惯了标题百分比] → 明细面板一点即达；红点保留了"需要看一眼"的信号。
- [删除 AK/SK 后，没用 arkcli 的人失去火山数据源] → 已与用户确认：arkcli 是必须前提，该场景不存在。
- [旧配置文件中 show_* 键残留] → serde 忽略未知字段；下次保存配置时自然重写干净。
