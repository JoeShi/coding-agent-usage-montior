# ark-agent-plan-usage Delta

## MODIFIED Requirements

### Requirement: arkcli 登录态 fallback

当用户未配置 AK/SK 时，系统 SHALL 检测本机是否存在 arkcli 的 SSO 登录态（`~/.arkcli/identities/<identity>/sts.json` 中的 STS 临时凭证），若存在且未过期（含 60 秒安全余量），则复用该 STS 凭证（含 session token）进行 V4 签名查询。`~/.arkcli/.env` 中的 VOLCENGINE_STS_* MUST NOT 作为凭证来源——该文件只在 login 时写入，静默续期不更新，恒为过期死值。

当 sts.json 已过期或 60 秒内将过期时，系统 SHALL 通过运行 `arkcli auth status --format json` 触发 arkcli 的静默续期（arkcli 在 SSO identity 有效时会自动刷新 STS，无需浏览器与用户交互），随后重读 sts.json。仅当续期失败或重读后凭证仍过期（identity 真正失效）时，才将数据源标记为需要重新登录，提示用户运行 `arkcli auth login` 或改配 AK/SK。

续期子进程 MUST 受控：超时上限 30 秒（超时强杀）、90 秒冷却防抖、串行化执行防止并发重复拉起；仅在 `~/.arkcli/identities/` 目录存在时才尝试续期；找不到 arkcli 可执行文件时静默跳过续期并按原有逻辑判定。凭证解析为同步可能阻塞操作，异步查询路径 MUST NOT 因续期子进程阻塞 async runtime。

#### Scenario: 零配置使用 arkcli 登录态

- **WHEN** 用户未配置 AK/SK，且本机 arkcli 登录态有效（sts.json 未过期）
- **THEN** 系统自动复用该登录态完成查询，界面标注凭证来源为 arkcli

#### Scenario: STS 过期后自动续期

- **WHEN** sts.json 已过期，但 arkcli 的 SSO identity 仍有效
- **THEN** 系统运行 `arkcli auth status --format json` 触发静默续期，重读 sts.json 获得新凭证并完成查询，全程无需用户操作、不提示重新登录

#### Scenario: arkcli 凭证过期

- **WHEN** sts.json 已过期，且触发静默续期后仍无法获得有效凭证（identity 已失效、续期命令超时或失败）
- **THEN** 系统将数据源标记为需要重新登录，提示用户运行 `arkcli auth login` 或改配 AK/SK

#### Scenario: 未安装或未登录 arkcli

- **WHEN** 本机不存在 `~/.arkcli/identities/` 目录，或找不到 arkcli 可执行文件
- **THEN** 系统不拉起任何子进程，按未配置 / 凭证过期的既有逻辑处理

#### Scenario: 续期子进程防抖

- **WHEN** 距上一次续期尝试不足 90 秒，或上一次续期已超时
- **THEN** 系统不重复拉起子进程，直接使用当前可读到的凭证状态判定
