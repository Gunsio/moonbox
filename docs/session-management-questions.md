# Session 管理问题

## Session 身份

- 一个 session 的稳定身份到底是什么：目标、工作区、分支、用户意图、agent，还是 transcript？
- 什么情况下，一个 session 已经不再是原来的 session？
- 当 session 方向变化时，Moonbox 应该保留什么？
- 一个不再活跃的 session，什么地方仍然有价值？

## Resume

- 什么时候 resume 比重新开始更便宜？
- 哪些信号说明当前上下文仍然可信？
- 哪些信号说明一个 session 已经过期？
- resume 前应该暴露多少未完成状态？
- 即使 source CLI 支持 resume，Moonbox 什么时候应该劝用户不要 resume？

## Fork

- 用户想探索新方向时，正确的 fork 点在哪里？
- 什么时候 fork 比 handoff 更合适？
- 多条推理分支应该如何比较？
- fork 应该继承哪些上下文，又应该丢掉哪些上下文？
- Moonbox 应该如何表达一个 fork 只是探索分支，而不是权威结论？

## Rewind

- 什么样的历史 turn 才是有意义的 rewind 点？
- Moonbox 如何发现后续上下文已经被错误假设污染？
- rewind 时，什么时候应该保留后续证据？
- 什么时候 rewind 比在当前 session 里手动纠偏更安全？
- Moonbox 应该如何向用户解释 rewind 的成本？

## Handoff

- 什么情况下，一个 session 已经适合 handoff？
- 另一个 agent 不重新打开 source session 也能继续时，handoff 里必须包含什么？
- handoff 里绝对不应该包含什么？
- Moonbox 如何判断一个 handoff 很弱？
- 什么时候 handoff 比 fork 或 resume 更合适？

## Abandon

- 什么时候一个 session 已经不值得继续维护？
- 哪些信号说明重新开始比修复上下文更便宜？
- Moonbox 应该如何从被 abandon 的 session 里保留有用证据？
- 被 abandon 的 session 应该如何保持可搜索，同时又不鼓励继续复用？

## Guidance

- Moonbox 什么时候应该推荐动作，而不是只提供动作？
- Moonbox 给出建议前需要哪些证据？
- 不确定性应该如何展示，才不会变成噪音？
- 哪些建议必须要求用户确认？
- Moonbox 如何避免把启发式判断包装成误导性的权威？

## Risk

- 哪些 session 操作是可逆的？
- 哪些 session 操作可能丢失重要上下文？
- resume、fork、rewind、handoff 之间会泄漏哪些隐私或敏感上下文？
- Moonbox 应该如何区分 source session 的事实和生成出来的摘要？
- 当 transcript、workspace、GitHub、外部文档互相冲突时，Moonbox 应该如何处理？

## Evaluation

- 如何判断一次 session 管理建议是正确的？
- 哪些回归只能通过真实工作流 review 发现？
- fixture 测试应该证明什么，哪些必须留给人工或视觉 review？
- 用户的什么行为能说明 Moonbox 降低了 session 管理摩擦？
- 每个里程碑结束后，还剩哪些问题没有被回答？
