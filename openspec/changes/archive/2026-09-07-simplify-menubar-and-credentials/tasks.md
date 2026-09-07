# Tasks: simplify-menubar-and-credentials

## 1. 状态栏纯图标

- [x] 1.1 删除 `lib.rs` 的 `tray_title()`、`source_tag()` 及 `tray_title_*` 三个单测；`update_tray` 改为只 `set_icon` + `set_title(None)`；`cargo test` 通过
- [x] 1.2 删除 `config.rs` 的 `show_kimi/show_ark/show_kiro` 字段及 `lib.rs`/`App.tsx` 中对它们的全部引用（含设置窗口"状态栏显示"section）；确认旧配置文件含 show_* 键时反序列化不报错（serde 忽略未知字段）；`cargo check` 无新增警告、`npm run build` 通过

## 2. 火山凭证收敛为 arkcli 单路径

- [x] 2.1 `credentials.rs`：删除 `choose()`、`keychain_aksk()`、`save_ark_aksk()`、`clear_ark_aksk()`、`ArkCredSource` 枚举及 `ArkCredentials.source` 字段，`resolve_ark_credentials()` 直接返回 arkcli 路径结果；更新受影响单测；`cargo test` 通过
- [x] 2.2 `lib.rs`：删除 AK/SK 保存/清除/状态查询相关 command（含保存时的验证查询逻辑），确认前端无对应 invoke 残留；`cargo check` 无新增警告
- [x] 2.3 `providers/ark.rs`：删除 `credential_source` extra 及其填充代码；`cargo test` 中 ark 相关测试通过
- [x] 2.4 `App.tsx`：删除"火山引擎凭证"整个 section 及相关 state/handler（ak/sk/cred/msg/saveCreds/clearCreds 等）；NeedRelogin 文案去掉"或配置 AK/SK"；`npm run build` 通过

## 3. 删除 AFPDaily

- [x] 3.1 `providers/ark.rs`：删除 `AfpResult.daily` 字段与 `push("daily", ...)`；测试 fixture 的 JSON 中保留 AFPDaily 以验证忽略行为，断言窗口列表只含 5h/weekly/monthly；`cargo test` 通过

## 4. 验证

- [x] 4.1 `cd src-tauri && cargo test` 全部通过且 `cargo check` 无新增警告；`npm run build` 通过
- [x] 4.2 手动验证：`cargo tauri dev` 启动后状态栏只显示圆点图标（无文本），明细面板三个数据源正常，火山窗口只有 5h/weekly/monthly，设置窗口无"火山引擎凭证"与"状态栏显示"section
