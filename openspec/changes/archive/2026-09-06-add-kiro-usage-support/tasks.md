# Tasks

## 1. Kiro provider

- [x] 1.1 在 `model.rs` 的 `DataSource` 增加 `KiroCli`，`SourceExtras` 增加 Kiro 字段（订阅档位、overage 状态/额度/费用、邮箱），验证 serde 测试通过
- [x] 1.2 实现 `src-tauri/src/providers/kiro.rs`：`GetUsageLimits` 调用（Bearer + `tokentype: API_KEY` + UA 伪装），防御式解析 subscriptionInfo/usageBreakdownList(CREDIT)/overageConfiguration，用参考实现的真实响应样例做解析单元测试
- [x] 1.3 错误映射：鉴权错误（401/403/token invalid）→ AuthError，网络错误 → Stale（保留旧数据），未配置 → NotConfigured，单元测试覆盖

## 2. 凭证管理

- [x] 2.1 `credentials.rs` 增加 Kiro API Key 的 Keychain 存取与清除（`kiro_api_key`），probe 增加自清理的往返验证
- [x] 2.2 新增 `save_kiro_credentials` 命令：保存前先调 `GetUsageLimits` 验证，成功才写 Keychain 并触发刷新；失败返回服务端错误且不写入。验证：设置窗口输入真实 ksk_ key 保存成功并显示订阅档位

## 3. 调度与托盘

- [x] 3.1 `config.rs` 增加 `show_kiro`（默认 true），验证旧配置文件（无该字段）加载兼容
- [x] 3.2 `refresh_all` 聚合 Kiro provider；托盘标题加 `R` 标签，异常显示 `R:--`，验证三数据源并存时标题格式正确
- [x] 3.3 验证 ≥80% 阈值加速对 Kiro 月度窗口同样生效（单元测试覆盖即可）

## 4. 前端

- [x] 4.1 用量页增加 Kiro 卡片：月度窗口进度条、订阅档位、overage 信息、账号邮箱、数据源状态
- [x] 4.2 设置页增加 Kiro API Key 输入（保存并验证 / 清除）与 `show_kiro` 开关，验证保存后状态栏出现 R 标签

## 5. 集成验证

- [x] 5.1 端到端：用真实 ksk_ key 验证查询、保存、清除、错误 key 提示全链路
- [x] 5.2 `cargo test` 全绿 + `openspec validate add-kiro-usage-support --strict` 通过
