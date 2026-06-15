# Session Action Roadmap

这份路线图只做任务拆分，不做实现。每个实现 milestone 仍然遵守
one branch、one commit、one PR。

## M105 Session Action Foundation

目标：先统一每个 session 的可用动作模型，再扩展 UI。

TODO:

- 定义统一 action model，覆盖 Inspect、Resume、Jump、Fork、Handoff、Archive 和后续动作。
- 为 action 表达 available、unavailable、blocked、warning 状态、面向用户的原因和安全约束。

验收:

- TUI 和 CLI 都能从同一个模型查询 session 可用动作。
- blocked action 能解释阻塞原因。
- Header、footer、action menu、快捷键都能消费同一个 action model。
- 测试不读取、不打开、不修改真实 recent sessions。
- 所有需要写状态的动作只写 Moonbox overlay，不写 provider 原始 session store。
- 不在本 milestone 中实现 primary / next action 选择；该逻辑进入 M110。

## M106 Archive Overlay

目标：让 Archive / Unarchive 在所有 provider 上体验一致。

TODO:

- 像 star 一样，把 archive 状态存到 Moonbox 本地 overlay。
- 默认列表隐藏 archived sessions。
- 支持 archived / all filter，并保持搜索可用。
- TUI 和 CLI 都支持 archive / unarchive。
- 不写入 Codex、Claude、Hermes 或其他 source store。

验收:

- Codex 和非 Codex session 的 archive UX 一致。
- archived session 仍可搜索、可恢复。
- fixture 测试覆盖 archive、unarchive、filter、source-store read-only 行为。

## M107 Whole Session Fork

目标：先支持整个 session fork，不做 turn-level fork。

TODO:

- 从选中 session 生成 whole-session fork context。
- 打开目标 agent 承接这份 fork context。
- 把 fork 作为独立 action，不混进 handoff。
- 在 launch plan 中记录 fork source metadata。
- rewind-point fork 等 fork / handoff 模型稳定后再做。

验收:

- 用户可以把整个 session fork 到新的目标 agent。
- fork context 启动前可 review。
- fork 不修改 source session。

## M108 Hook-Backed Live State Completion

目标：补完 live-state，让 Moonbox 区分 active session 和 resumable session。

TODO:

- 用 hook 数据判断 source process 是否 alive。
- 判断 alive process 是否 attached to 可定位的 tmux pane。
- 只有 alive + tmux pane 可定位时才显示 Jump。
- Jump 不可靠时显示 Resume / Fork / Handoff。
- hooks 保持 opt-in 和 fail-open。

验收:

- 选中 session 能看到 alive 和 tmux 状态。
- 无法安全定位进程时不显示 Jump。
- dead 或 unknown session 回落到普通 session actions。

## M109 Header / Footer Information Architecture

目标：让全局 header 和选中 session footer 承载高价值信息。

TODO:

- Header 保持全局：data source、filter、theme、默认 target、默认 skill、runner、hook/live-state 是否启用。
- Footer 跟选中 session 变化：alive、tmux、cwd、branch、primary action placeholder、action keys、blocked reason、warning。
- 移除低价值或重复状态文本。
- 对窄屏和宽屏终端做视觉 review。

验收:

- Header 能回答“Moonbox 当前处于什么模式？”
- Footer 能回答“当前 session 当前状态是什么、可做动作是什么？”
- 常见终端宽度下文本不换行错位、不重叠。

## M110 Deterministic Primary Action

目标：单独定义低风险、可解释的默认主动作，不把它混进 action model 基础层。

TODO:

- 定义 deterministic primary action v0，而不是 AI/heuristic recommendation。
- 用 live state、provider resume capability、source read-only 状态、archive 状态决定主动作。
- 初始规则：alive + tmux pane 可定位时是 Jump；否则可恢复时是 Resume；否则可生成上下文时是 Handoff；archived 时是 Unarchive 或 Inspect；无法安全判断时回落到 Inspect。
- Fork 不作为默认主动作，只在用户明确选择探索或分支时出现。
- 在 footer 中显示 `Next: <action>` 和原因。

验收:

- 每个 session 最多有一个 deterministic primary action。
- primary action 有可解释原因，不依赖 AI 判断。
- 不确定或状态不足时回落到 Inspect。

## M111 Built-In Handoff Skill

目标：提供一份 first-party handoff skill，同时保留三方 skill 支持。

TODO:

- 学习社区高质量 handoff skill，提炼可复用结构。
- 增加内置 Moonbox handoff skill。
- 保持 skill-first，输出仍然是可 review 的生成 artifact。
- Moonbox 专属 sections 只在有实际 continuation 价值时加入。
- 保留现有三方 skill 选择路径。

验收:

- 干净安装后有可用的默认 handoff skill。
- 三方 handoff skill 仍可选择。
- 内置 skill 输出足以让新 agent 不重新打开 source session 也能继续。

## M112 Action Menu and Shortcut Rebalance

目标：功能变多后，避免裸快捷键膨胀。

TODO:

- 增加 `o` action menu，覆盖 Inspect、Resume、Jump、Fork、Handoff、Archive 等动作。
- 高频安全动作保留直达快捷键。
- 危险或低频动作进入二级菜单。
- 在能降低歧义的地方支持组合键或分组快捷键。
- 更新 key hints 和测试。

验收:

- 新用户可以通过 `o` 发现动作。
- 老用户常用路径仍高效。
- 危险动作不会因误触裸单键触发。

## M113 Settings and Setup Closure

目标：把持久化偏好和安装类配置在 Moonbox 内闭环。

TODO:

- Settings 分为 User Preferences、Workflow Defaults、Integrations / Setup。
- 持久化默认 target agent、handoff skill、runner、archive 行为、fork target、是否显示 archived sessions。
- 展示 hooks、runner SDK、skills、tmux 检测、provider binary path 的 install/status 流程。
- 外部安装或 setup 命令必须 preview + confirm。

验收:

- 重复工作流不需要每次重新选择默认项。
- 缺失 setup 能在 Moonbox 内解释并修复。
- 安装动作不会静默执行。

## M114 Luoshen Theme Polish

目标：继续雕琢四个洛神赋主题。

TODO:

- 在真实 TUI 页面 review 四个 Luoshen themes。
- 优化 contrast、hierarchy、selected row、warning、blocked states。
- 检查 truecolor 和 ANSI fallback。
- 主题改动保持在 semantic tokens 内。

验收:

- 每个主题都清晰、可读、有区分度，并达到产品级水准。
- 重要状态在每个主题下都明确可见。
- 截图或 render tests 覆盖主题关键页面。

## M115 Product Quality Audit

目标：核心 session actions 成型后，重新审视代码是否仍保持顶级开源水准。

TODO:

- Review architecture、state ownership、naming、tests、user-facing flows。
- 找出快速迭代中积累的 accidental complexity。
- 清理重复 action logic 和过时 UI path。
- 审计 fixture safety 和 source-store read-only 保证。
- 输出按优先级排序的 follow-up list，而不是做泛泛大重构。

验收:

- 代码库仍满足 top-tier open-source quality 标准。
- 已知质量风险明确、可排序。
- 重构范围基于证据，不做审美性 churn。
