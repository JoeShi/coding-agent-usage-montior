# menubar-display Delta

## MODIFIED Requirements

### Requirement: 下拉明细视图

系统 SHALL 在用户打开状态栏明细视图时，以 macOS 原生 popover 风格的面板展示：半透明毛玻璃背景（随系统深浅色变化）、圆角、锚定在状态栏图标正下方、不可拖动。面板内容为各数据源的完整明细：每个滚动窗口的已用量/配额/百分比/重置时间、Kimi Code 的会员等级与 Extra Usage 余额、火山套餐档位、Kiro 的订阅档位与超额计费信息（超额开关/超额 credits/预估费用），以及各数据源的状态（正常 / 数据过期 / 鉴权错误 / 未配置）。

#### Scenario: 查看完整明细

- **WHEN** 用户点击状态栏图标打开明细面板
- **THEN** 面板以毛玻璃 popover 形态出现在图标正下方，列出每个数据源的所有滚动窗口明细、重置时间和数据源状态

#### Scenario: 面板不可拖动

- **WHEN** 用户尝试拖动明细面板
- **THEN** 面板保持锚定位置不移动

#### Scenario: 查看 Kiro 超额信息

- **WHEN** Kiro 账号开启了超额计费且产生了超额用量
- **THEN** 明细视图展示超额 credits 与预估美元费用

## ADDED Requirements

### Requirement: 弹窗失焦自动收起

明细面板 SHALL 在失去焦点（用户点击面板之外的任意位置）时自动收起，不需要用户手动关闭。

#### Scenario: 点击外部收起

- **WHEN** 明细面板已打开且用户点击面板之外的区域
- **THEN** 面板自动隐藏

#### Scenario: 再次点击图标切换

- **WHEN** 明细面板已打开且用户再次点击状态栏图标
- **THEN** 面板收起
