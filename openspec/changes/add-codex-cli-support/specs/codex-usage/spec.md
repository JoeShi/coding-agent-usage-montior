## Purpose

让使用 ChatGPT 套餐登录 Codex CLI 的用户在不向本应用复制任何 OpenAI 凭证的前提下，查看 Codex 各限额桶的滚动窗口用量、重置时间、套餐信息和可操作的数据源状态。

## ADDED Requirements

### Requirement: 安全复用 Codex CLI 登录

系统 SHALL 通过本机 Codex 提供的账号接口复用其当前登录状态，并 MUST NOT 直接读取、复制、记录或持久化 Codex 的访问令牌、刷新令牌、API Key 或完整账号响应。系统 SHALL 兼容 Codex 将凭证保存在文件或操作系统凭证存储中的方式。

#### Scenario: 复用 ChatGPT 登录

- **WHEN** 用户已通过 Codex CLI 使用 ChatGPT 账号登录
- **THEN** 系统无需用户再次输入凭证即可查询 Codex 套餐限额

#### Scenario: 凭证存储方式变化

- **WHEN** Codex 将登录凭证从本地文件切换到操作系统凭证存储
- **THEN** 系统仍通过 Codex 账号接口查询限额，不依赖 `auth.json` 的存在或结构

#### Scenario: 仅使用 API Key 登录

- **WHEN** Codex 当前仅使用 OpenAI Platform API Key 登录
- **THEN** 系统将 Codex 数据源标记为未配置，并提示 ChatGPT 套餐限额监控需要使用 ChatGPT 登录，且不把 Platform 账单或速率限制冒充套餐限额

### Requirement: 查询并规范化 Codex 套餐限额

系统 SHALL 查询 Codex 当前账号及 ChatGPT 套餐限额。对于服务端返回的每个限额桶，系统 SHALL 将主窗口和次窗口分别规范化为用量窗口，包含已用百分比、总量 100、窗口时长和下一次重置时间；系统 SHALL 同时展示可用的套餐类型。

#### Scenario: 返回默认限额桶

- **WHEN** Codex 返回包含主窗口和次窗口的默认限额桶
- **THEN** 系统分别展示两个窗口的已用百分比、窗口时长和重置时间

#### Scenario: 返回多个限额桶

- **WHEN** Codex 返回按 `limitId` 区分的多个限额桶
- **THEN** 系统展示每个限额桶的所有有效窗口，并使用服务端名称或稳定的桶标识区分它们

#### Scenario: 仅返回兼容单桶视图

- **WHEN** Codex 未返回多桶视图但返回兼容的单桶限额
- **THEN** 系统使用该单桶限额生成窗口，不重复展示同一数据

#### Scenario: 可选窗口缺失

- **WHEN** 某个限额桶没有次窗口或其他可选字段
- **THEN** 系统展示仍然有效的窗口并省略缺失字段，不将缺失值伪造为零用量

### Requirement: Codex 数据源状态可诊断

系统 SHALL 将 Codex CLI 或账号接口的结果映射为现有数据源状态，并提供不含敏感信息的可操作提示。Codex CLI 不存在、账号接口不受支持或尚未登录时 SHALL 标记为未配置；登录凭证无法恢复时 SHALL 标记为需要重新登录；权限拒绝时 SHALL 标记为鉴权错误；进程超时、协议异常或临时服务失败时 SHALL 标记为数据过期。

#### Scenario: Codex CLI 未安装

- **WHEN** 系统无法找到 Codex CLI 可执行文件
- **THEN** Codex 数据源显示未配置并提示安装 Codex CLI

#### Scenario: CLI 版本不支持账号限额接口

- **WHEN** Codex CLI 可以启动但不支持所需账号限额方法
- **THEN** Codex 数据源显示未配置并提示升级 Codex CLI

#### Scenario: ChatGPT 登录失效

- **WHEN** Codex 无法刷新或恢复当前 ChatGPT 登录
- **THEN** Codex 数据源显示需要重新登录并提示运行 Codex 登录流程

#### Scenario: 临时读取失败

- **WHEN** Codex 账号进程超时、输出无效或服务临时失败
- **THEN** Codex 数据源显示数据过期，保留上一次成功窗口并在下一个轮询周期重试

### Requirement: Codex 窗口参与统一监控行为

系统 SHALL 将成功获取的 Codex 窗口纳入现有的明细展示、80% 高用量警示、加速轮询和手动刷新行为，不为 Codex 创建独立调度器或不同阈值。

#### Scenario: Codex 窗口达到告警阈值

- **WHEN** 任一 Codex 窗口的已用百分比达到或超过 80%
- **THEN** 状态栏显示高用量警示，系统按现有规则切换到加速轮询

#### Scenario: 用户手动刷新

- **WHEN** 用户触发现有的立即刷新入口
- **THEN** 系统与其他数据源一起刷新 Codex 用量并更新明细面板
