# naicode OAuth、欢迎框与模型选择器改进设计

日期：2026-07-12

## 1. 目标

本轮完成三项相互独立但共同影响 naicode 日常使用体验的改进：

1. 修复 Relay OAuth rotating refresh token 并发刷新造成的误报：正常登录状态不得频繁显示 `reauthentication_required`。
2. 保留当前 `NAICODE` ASCII 艺术字的字符、间距和尺寸，仅在现有欢迎内容外围增加终端可真实渲染的边框。
3. 重新整理 `/model` 的终端布局，并在每个分组名称后显示服务端返回的最终有效倍率。

本轮只修改 naicode 客户端。不得推送远端；除非后续发现必须修改服务端且用户另行授权，否则不得部署 new-api。

## 2. 视觉基准

### 2.1 精确产品蓝色

复用本机 Claude Code 状态行目录段 ` AndroidStudioProjects` 的确切颜色：

```text
背景色  #2563EB  RGB(37, 99, 235)
前景色  #DBEAFE  RGB(219, 234, 254)
```

颜色来源是 `C:\Users\pc\.claude\statusline.ps1` 中目录段的 `bgc/fgc` 参数，不使用视觉近似值。

naicode 的默认产品主色、聚焦边框和主要选中背景以 `#2563EB` 为基准；选中前景使用 `#DBEAFE`。ANSI-256 与 ANSI-16 终端继续经过现有终端色级降级逻辑，不能假定所有终端都支持 TrueColor。

用户自定义 `[tui].product_accent` 的能力继续保留。内置默认色改为上述精确蓝色；自定义色仍由语义色构造函数派生其余颜色，但不得影响默认色必须精确复用的要求。

### 2.2 终端真实性约束

所有视觉效果必须由 Ratatui 的字符单元、Unicode box-drawing 字符、前景色、背景色和文本 modifier 直接实现。禁止引入只能存在于网页预览中的圆角半径、阴影、渐变、半透明、自由像素定位或非等宽布局。

浏览器视觉伴侣如用于验收，只能作为等宽终端网格模拟器；最终代码中的字符、空格、边框、换行和颜色必须与预览一致。

## 3. 欢迎区设计

### 3.1 保留现有艺术字

当前欢迎区中的 `NAICODE` ASCII 艺术字必须原样保留：

- 不替换字形；
- 不改变组成字符；
- 不改变艺术字内部间距；
- 不改变艺术字尺寸；
- 不为了适配边框而重新绘制 Logo。

本次只为现有欢迎区增加外围终端边框，并继续保留现有品牌、版本、模型、分组、思考等级、工作目录和帮助提示。

### 3.2 边框和布局

欢迎区使用一层 Unicode 终端边框：

```text
╭──────────────────────────────────────────────────────╮
│                                                      │
│   [现有 ASCII 艺术字原样渲染]                        │
│                                                      │
│   [现有品牌和会话信息]                               │
│                                                      │
╰──────────────────────────────────────────────────────╯
```

边框使用产品蓝 `#2563EB`。艺术字继续使用现有语义主色和强调色，不将全部正文填充为蓝色背景。

宽度适配遵循以下规则：

- 宽终端：完整显示当前艺术字和会话摘要；
- 中等终端：艺术字保持原样，摘要允许按既有规则分行；
- 宽度不足以容纳当前艺术字及边框时：使用当前实现已有的紧凑回退形式，不截断边框、不把艺术字重新设计为另一套字形；
- 当可用宽度小于 4 列或高度小于 2 行时不绘制欢迎框；宽度可容纳边框但不能容纳艺术字时，只显示当前既有紧凑品牌行，完整艺术字本体不裁剪；高度不足时按“边框闭合优先”依次省略帮助、摘要和空白行；动态 resize 每帧重新计算，不沿用旧区域；
- 所有行必须在显示宽度内闭合边框，中文、Nerd Font 和宽字符宽度按终端显示宽度计算，不能使用 UTF-8 字节数估算。

实现前把当前 ASCII 艺术字的精确逐行字符串固定为测试 fixture/snapshot，包括行内空格、行尾空格和换行；新增的外围 padding 与边框不属于艺术字 fixture。测试同时保存艺术字 fixture 的稳定哈希，防止快照更新时无意接受字形变化。

fresh session 的展示顺序保持：欢迎框 → Relay 公告/tooltip → 帮助内容。`/clear`、resume、fork 和 replay 不重复大欢迎框。

## 4. `/model` 模型选择器设计

### 4.1 信息架构

保留当前“左侧分组、右侧模型、底部详情”的结构，不重写键盘交互模型：

- 左侧：分组名称和有效倍率；
- 右侧：当前分组内的模型，每个模型固定两行；
- 底部：选中模型的低频价格详情；
- 最底部：键盘操作提示。

模型选择后继续进入现有思考等级选择弹层。

### 4.2 分组倍率

每个分组名称后显示其最终有效倍率，例如：

```text
▸ default  ×1
  vip      ×0.8
  coding   ×1.25
```

倍率必须来自 catalog 对该用户与目标分组返回的最终有效 `group_ratio`，客户端不得根据账户组、目标组或价格反向推算。DTO 应优先使用服务端规范化 decimal 字符串；若现有协议只能提供 JSON number，则解析为十进制定点/任意精度 decimal 后再格式化，禁止直接用二进制浮点的调试输出。

格式规则：

- 使用 `×` 前缀；
- 合法值必须为有限、非负十进制；负数、NaN、Infinity、解析失败均显示 `×—`；零显示 `×0`；
- 去掉无意义的小数末尾零；
- `1.0` 可显示为 `1`，`0.80` 显示为 `0.8`；
- 最多展示 6 位小数；超过 6 位按 decimal 的 round-half-even 规则显示，避免浮点噪声；
- 倍率文本（不含 `×`）最长 10 个终端列；超过可显示范围时使用科学计数法，仍超过则显示 `×超范围`；该状态必须完整可见；
- 缺失倍率显示 `×—`，不得默认为 `×1`；
- 分组名过长时优先中间省略分组名，倍率保持完整可见；
- 选中模型的底部详情标题同时显示当前分组及倍率。

如果当前 Rust DTO 只在模型价格对象中携带 `group_ratio`，渲染层应为当前 catalog 建立“分组 → 有效倍率”的只读视图；同一分组出现不一致倍率时不得静默挑选，视为 catalog 数据不一致并显示 `×—`。诊断仅记录分组标识、已脱敏的 catalog/version 标识和冲突倍率，不得记录 access/refresh token、Authorization header、完整响应、用户个人信息或其他凭据；相关日志测试必须验证敏感字段不会出现。

### 4.3 宽度布局

#### 宽屏（`>= 96` 列）

- 左侧固定分组栏，宽度需容纳常见分组名和倍率；
- 右侧模型名占第一行；
- 第二行显示输入、输出、缓存读取、缓存创建等高频价格；
- 底部详情显示 1h 缓存、图片、音频、按次等低频通道。

#### 中屏（`72..=95` 列）

- 保留左右分栏；
- 价格折为较紧凑的两列或符号标签；
- 分组倍率仍显示在分组名后，不因宽度缩小而删除；
- 分组名必要时省略，但倍率不能被截断。

#### 窄屏（`< 72` 列）

- 分组栏收起为顶部当前分组行；
- 宽度足够时显示 `分组：<名称>  ×<倍率>`；不足时先让倍率单独占下一行，再省略分组名；倍率仍放不下时显示最短状态 `×—`，不输出半个宽字符；
- 模型价格改为单列；
- 保留切换分组的现有键盘入口，并在帮助行中明确提示；
- 当宽度小于 4 列或高度小于 2 行时不绘制 picker；其余高度不足时按“选中模型可见优先”依次隐藏底部详情、帮助行、搜索提示和非必要价格行，边框必须闭合；
- 动态 resize 每帧重新计算布局、可见卡片数和 scroll window，不能沿用旧宽高下的区域或产生越界。

### 4.4 颜色与状态

- 当前选中项背景：`#2563EB`；
- 当前选中项前景：`#DBEAFE`；
- 当前焦点边框：`#2563EB`；
- 未聚焦边框：语义暗灰色；
- 价格文字不使用调试风格的黄色/青色大面积混排；
- Loading、空列表、网络错误、认证错误继续使用彼此独立的状态视图；
- 缺失价格显示 `—`，不显示 `$-`、`¥-` 或推测值。

## 5. Relay OAuth 并发刷新设计

### 5.1 根因

当前 `AuthManager::shared()` 每次创建新的 `Arc<AuthManager>`，而 `refresh_lock` 属于单个 manager。多个 manager 可能同时读取相同 rotating refresh token：第一个刷新成功并轮换 token，第二个仍使用旧 token 刷新，随后把 `401` 或 `invalid_grant` 永久映射为 `ReauthenticationRequired`。

### 5.2 共享刷新协调器与跨进程互斥

引入两层 Relay refresh 协调：

- **进程内 registry**：按凭据存储身份复用 coordinator，使同一进程中的不同 `AuthManager` 共享 singleflight；
- **跨进程互斥**：在真正调用 refresh endpoint 前获取同一 storage identity 对应的 OS 级互斥锁。Windows 使用命名 mutex 或具备自动释放语义的等价锁；其他平台使用锁文件或现有跨平台锁实现。进程异常退出后锁必须由 OS 自动释放，不能留下永久死锁；
- 获取跨进程锁后必须再次 reload storage 并比较 snapshot，因此等待另一个进程完成轮换的请求不会再次消费旧 refresh token；
- 锁只覆盖 reload → refresh → durable persist，不覆盖原始模型网络请求；
- 不同 storage identity 之间不得互相阻塞。

storage identity 使用明确且不含秘密的算法：

1. 文件后端以 `codex_home` 的绝对、词法规范化路径为基础；Windows 统一目录分隔符并按大小写不敏感比较，能解析现存路径时进一步使用 canonical path，以合并 junction/符号链接别名；
2. 默认 home 与显式指向同一路径必须得到同一 identity；
3. keyring 后端使用固定应用 service 名、账户槽位和规范化 `codex_home` 的组合；
4. registry key 和 mutex 名只保存上述 identity 的稳定哈希，绝不包含 access token、refresh token、Authorization header 或其他秘密；
5. registry 使用 `Weak` 引用或等价清理，避免废弃 home 永久占用内存。

### 5.3 刷新流程

每次授权请求保存初始 Relay OAuth snapshot，至少包含：

```text
credential_generation（若现有存储没有，则随本次改造加入）
account/session identity
access_token
refresh_token
expires_at
```

reload 后的凭据只有在记录完整、generation 单调增加、账户身份未变且 session 与原请求兼容时，才可视为另一刷新者发布的新凭据。若账户或 session 被切换、凭据字段不完整、generation 倒退，原请求必须中止并返回“认证状态已变化”或“凭据存储损坏”，不得用新账户身份静默重放。

`credential_generation` 是整个完整凭据记录的持久化版本，规则如下：

- 旧格式记录首次成功加载时视为 generation `0`；下一次任何凭据写入以 generation `1` 的完整新格式记录迁移；
- refresh、登录、登出、换账户以及任何改变认证字段的完整记录提交都必须递增 generation；只读和非认证缓存更新不得递增；
- 写入者在锁内读取当前 generation，并以 `current + 1` 提交；文件后端在跨进程锁内完成，keyring 后端也必须使用相同 storage identity 的跨进程锁；
- durable commit 前再次验证正式记录 generation 仍等于读取值；不相等则放弃本次写入、reload 并走身份一致性判断，禁止两个写者发布相同 generation；
- generation 使用无符号 64 位整数；达到上限时拒绝写入并返回明确存储错误，不回绕；
- 登出可以提交不含 token 但带递增 generation 和“logged_out”状态的完整墓碑记录，使等待者能识别注销，而不是把字段缺失误当作损坏。

需要刷新时：

1. 获取该 storage identity 对应的进程内 coordinator 锁，再获取跨进程互斥锁；
2. 锁内从真实 storage backend（文件或 keyring）重新加载并验证完整凭据记录；
3. 如果 generation 增加、同一身份下 token snapshot 已变化且记录完整，说明其他请求已刷新，直接使用新 access token 重试，不再调用 refresh；
4. 只有 snapshot 未变化且 token 临近过期或请求收到 401 时才调用 refresh endpoint；
5. refresh 成功后，先以一个完整凭据记录 durable commit，再更新 manager 内存状态。文件后端使用同目录临时文件、flush/fsync（平台支持时）和原子替换；keyring 后端若不能事务写多个字段，必须把完整凭据序列化为单个 keyring value。禁止逐字段发布可被其他进程观察到的新旧混合状态；
6. durable commit 失败时不得更新内存，不得删除旧记录；临时文件可在后续启动时安全清理。崩溃恢复只接受最后一个完整且可解析的正式记录，不从半写临时记录拼接凭据；
7. 原始请求最多重试一次，防止认证失败循环。

### 5.4 错误分类与恢复

refresh 返回 `401`、`invalid_grant` 或 `refresh_token_reused` 后不能立即提示重新登录。必须再次从真实 storage reload，并应用 5.3 节完全相同的完整性、generation、账户身份和 session 兼容性验证：

- 只有 reload 记录通过全部验证，且 generation 增加并携带同一身份的完整新 token snapshot，才视为并发刷新已由其他请求完成并使用新 access token 重试；
- 账户/session 改变时返回“认证状态已变化”，记录不完整或 generation 非法时返回“凭据存储损坏”，两者都禁止重放原请求；
- `refresh_token_reused` 或通用 `invalid_grant` 且 storage 未变化：将该 refresh token 的摘要标记为本进程不可再次使用，返回独立的 `RefreshConflictUnresolved`（或等价）可恢复错误；后续请求必须先 reload，只有 storage generation 增加后才能继续。不得自动循环刷新或持续冲击 refresh endpoint；UI 提示用户关闭其他 naicode 进程后重试，若状态持续不变再主动执行登录；
- 网络错误、超时、5xx 或响应格式错误：保留原错误类别，不标记 token 已消费；使用现有请求重试策略或用户手动重试，不在认证层无限退避循环；
- 只有服务端明确返回 `device_revoked`、`session_revoked` 或 `refresh_token_revoked`，且 reload 后 snapshot 仍未变化，才映射为 `ReauthenticationRequired`；
- 若旧服务端只返回通用 `invalid_grant` 而无明确撤销码，则不得武断永久判定登录失效，应返回可操作但非永久的刷新失败提示。

catalog、换组、`/v1/models` 和普通模型请求必须共享上述执行语义，不能存在直接读取磁盘 token 后自行 refresh 的旁路。

## 6. 组件边界

### `codex-rs/login/src/auth/manager.rs`

- coordinator registry 和 storage identity；
- snapshot 比较；
- 锁内 reload/refresh/retry；
- refresh 错误分类；
- 多 manager 并发测试。

如文件职责继续膨胀，可将纯协调结构提取到同一 auth 模块内的小文件，但不进行与本任务无关的认证重构。

### `codex-rs/tui/src/product_palette.rs`

- 将内置默认色更新为状态行精确色值；
- 维持自定义 accent 和终端色级降级；
- 为默认选中前景保留精确 `#DBEAFE`，并验证对比度。

### `codex-rs/tui/src/history_cell/session.rs`

- 保留现有 ASCII 艺术字；
- 在现有欢迎内容外围绘制边框；
- 处理终端显示宽度和紧凑回退；
- 保持 fresh session 与公告顺序。

### `codex-rs/tui/src/bottom_pane/relay_model_picker.rs`

- 渲染分组倍率；
- 调整分组栏宽度和名称省略；
- 更新宽、中、窄布局；
- 使用精确产品蓝；
- 保持两行模型和底部详情。

如 catalog DTO 尚不能稳定提供每组倍率，只做最小 DTO 扩展，不复制 new-api 的倍率计算规则。

## 7. 测试与验收

### 7.1 自动测试

OAuth：

- 两个不同 `AuthManager`、同一 `codex_home` 同时刷新，只允许一次真实 refresh；
- 两个独立测试进程使用同一 storage identity 并发刷新，只允许一次真实 refresh，另一进程 reload 后成功；
- 等待者 reload 新 token 后请求成功；
- 两个不同 `codex_home` 可独立刷新；
- Windows 大小写、相对/绝对路径、默认/显式 home，以及可用时 junction/符号链接别名映射到同一 identity；不同 identity 不碰撞；
- 旧格式 generation `0` 正确迁移；refresh、登录、登出和换账户均递增；并发提交不能发布相同 generation；上限不回绕；
- reload 出现账户或 session 改变时禁止重放原请求；不完整记录和 generation 倒退被识别为存储错误；
- `refresh_token_reused` 且 storage 已变化时恢复成功；
- `RefreshConflictUnresolved` 会阻止同一已消费 token 再次调用 refresh，只有 generation 增加后恢复；
- 通用 `invalid_grant` 不直接映射永久登录失效；
- 明确 revoked 错误且 storage 未变化时才返回 `ReauthenticationRequired`；
- refresh 成功但持久化失败时不得发布仅存在于内存的新 rotating token；
- 故障注入覆盖临时文件写入、flush、原子替换前后崩溃，以及 keyring 单记录提交失败；重启后只能读到最后一个完整正式记录；
- 原请求最多重试一次。

欢迎区：

- 当前 ASCII 艺术字内容与改动前完全一致；
- 宽、中、窄代表性宽度下边框闭合且无越界；
- 中文和宽字符不会造成右边框错位；
- 非 fresh session 不重复欢迎框。

模型选择器：

- 分组倍率格式化和缺失值；
- 分组名省略后倍率仍完整；
- catalog 同组倍率不一致时显示 `×—`；
- 96、72、71 列布局边界；
- 0–3 列、极小高度和动态 resize 时按规则隐藏或不绘制，无越界、残留旧区域或半个宽字符；
- 高度逐级不足时按指定优先级隐藏详情、帮助、搜索和价格行，选中模型保持可见；
- 选中颜色语义；
- 搜索、滚动、切组和思考等级衔接不回归。

产品色：

- TrueColor 默认值精确为 `#2563EB/#DBEAFE`；
- ANSI-256 和 ANSI-16 降级结果可用；
- 自定义 `product_accent` 继续生效。

### 7.2 本地真实验收

构建并安装本地 naicode 后，只进行一次代表性流程：

1. 新会话确认现有 ASCII 艺术字没有变化，外围边框闭合；
2. 打开 `/model`，确认每个分组后显示服务端倍率；
3. 在宽屏和小于 72 列的终端各检查一次布局；
4. 选择模型并进入思考等级弹层；
5. 让 catalog 与模型请求并发触发一次 refresh，确认不误报重新登录；
6. 使用明确撤销的测试会话确认真正失效时仍能提示重新登录。

若无法安全构造真实 token 临期或撤销场景，则必须如实标注该项只由确定性测试覆盖，不能声称已做真实验收。

## 8. 验收标准

- 正常并发刷新不再产生 `reauthentication_required` 弹层；
- 真正被明确撤销的会话仍返回清晰的重新登录提示；
- 欢迎区当前 ASCII 艺术字逐字符保持不变，只增加终端边框；
- 默认蓝色精确复用 `#2563EB`，选中前景为 `#DBEAFE`；
- `/model` 每个分组后显示服务端最终有效倍率，缺失时显示 `×—`；
- 模型选择器在宽、中、窄布局中均无破框、倍率截断或键盘交互退化；
- 不推送远端，不部署生产 new-api，不清理或覆盖仓库中既有未提交改动。
