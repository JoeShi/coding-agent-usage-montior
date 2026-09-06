# Tasks

## 1. 项目脚手架

- [x] 1.1 用 `npm create tauri-app` 初始化 Tauri v2 + React + Vite + TypeScript 项目，验证 `cargo tauri dev` 能启动空窗口
- [x] 1.2 添加 Rust 依赖（`reqwest`、`tokio`、`serde`、`keyring`、`hmac`、`sha2`、`chrono` 等），验证 `cargo build` 通过
- [x] 1.3 定义前后端共享的 `UsageSnapshot` / `QuotaWindow` / `SourceStatus` 数据模型（serde 序列化），编写单元测试验证序列化往返

## 2. 火山 V4 签名与 AgentPlan 数据源

- [x] 2.1 实现火山 V4 签名模块（HMAC-SHA256 派生链、canonical request、`X-Security-Token` 支持），用固定时间戳 + 已知凭证的签名向量做单元测试
- [x] 2.2 实现 `GetAFPUsage` 调用与响应解析（四窗口 Quota/Used/ResetTime/PlanType），用本机 arkcli STS 凭证做集成冒烟验证返回 200 且字段完整
- [x] 2.3 实现 `GetUsageDetails` 调用（Day/Hour 粒度、时间范围过滤）与解析（`Result.Details[]`：`BillingType`/`ObjectName`/`Time`/`Unit`/`Usage`，结构已实测确认，见 design.md 附录），用真实查询验证解析正确
- [x] 2.4 实现 arkcli 登录态 fallback：探测 `~/.arkcli/.env` 的 `VOLCENGINE_STS_*`，校验 `VOLCENGINE_STS_EXPIRES_AT_MS` 未过期后带 session token 签名；过期时返回 NeedRelogin 状态。验证：临时配置错误路径时正确返回 NotConfigured
- [x] 2.5 实现凭证优先级逻辑（显式 AK/SK 优先于 arkcli fallback）及鉴权失败 → AuthError 的状态映射，单元测试覆盖两种来源的切换

## 3. Kimi Code 数据源

- [x] 3.1 实现凭证文件读取与解析（`~/.kimi-code/credentials/kimi-code.json`），缺文件/解析失败返回 NotConfigured，单元测试覆盖
- [x] 3.2 实现 `GET /coding/v1/usages` 调用与响应解析（周配额、滚动窗口数组、会员等级、boosterWallet 余额），用本机真实 token 验证解析成功
- [x] 3.3 实现 token 刷新：过期检测 → refresh_token 请求 `https://auth.kimi.com/api/oauth/token` → 临时文件 + rename 原子写回（写回前重读比对 refresh_token），验证过期 token 能自动恢复查询
- [x] 3.4 实现 `invalid_grant` 竞争处理（重读文件重试一次，仍失败返回 NeedRelogin），用 mock HTTP 服务做单元测试

## 4. 轮询调度器

- [x] 4.1 实现 tokio 调度器：可配置基础间隔（默认 5min）、手动刷新 oneshot 通道，单元测试验证调度触发
- [x] 4.2 实现自适应加速（任一窗口 ≥80% 切 1min，回落恢复）与失败保留旧数据逻辑，单元测试覆盖阈值边界
- [x] 4.3 实现非敏感配置持久化（`~/.config/agent-plan-monitor/config.toml`：轮询间隔、显示偏好），验证重启后配置生效

## 5. Keychain 与凭证管理

- [x] 5.1 实现火山 AK/SK 的 Keychain 存取（`keyring` crate）与删除，验证应用数据目录无 Secret Key 明文
- [ ] 5.2 实现"保存即验证"逻辑：写入 Keychain 后执行一次 `GetAFPUsage` 验证并返回结果/错误原因，验证无效 AK 返回明确错误
- [x] 5.3 实现凭证来源查询与清除（清除后回退 arkcli 检测），单元测试覆盖状态流转

## 6. 状态栏与前端

- [x] 6.1 实现托盘图标与压缩文本标题（`K:90% A:45%`，异常 `--`，≥80% 变色），验证真实数据下状态栏显示正确
- [x] 6.2 实现原生托盘菜单（立即刷新 / 打开设置 / 退出），验证各入口行为
- [x] 6.3 实现明细视图窗口：各数据源窗口明细、重置时间、Kimi 会员等级与 Extra 余额、火山套餐档位、数据源状态标识，验证与快照数据一致
- [x] 6.4 实现设置窗口：AK/SK 输入与验证反馈、凭证来源展示、清除按钮、轮询间隔与显示偏好，验证保存后调度器与托盘行为随之改变

## 7. 集成验证

- [ ] 7.1 端到端验证：真实环境下 Kimi token 过期自动恢复、arkcli fallback 查询成功、AK/SK 路径查询成功
- [x] 7.2 运行 `openspec validate add-agent-usage-menubar-app --strict` 通过，并核对各 spec 的 Scenario 均有对应实现
