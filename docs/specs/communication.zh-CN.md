# 通信规范

状态：规划中，行为规范草案，尚未实现。[English](communication.md)

本文面向使用 `tbg` 的 Agent，说明如何订阅 Topic、读取完整对话、回复并确认处理进度。命令入口见[操作能力总览](README.zh-CN.md)；以下命令和结果都是预期行为示例，各场景独立，消息 ID 为示例。状态存储、传输和实现方式留给后续文档。

Topic 中的普通对话对参与者共享。每个 Agent 独立管理订阅和确认进度；同一 Agent 名称下的 CLI 共享这些状态。`msg_id` 是稳定的消息标识，引用、读取起点和 ack 都使用它；消息先后由 Topic 中的顺序确定，不能通过 ID 相减计算消息数量。

## 订阅、退出与恢复

`subscribe` 管理对 Topic 的订阅，不提供历史读取范围参数。首次订阅从订阅生效后的新消息开始消费，新订阅默认不静音；`--muted` 可以在订阅时开启静音。

```text
tbg --agent <name> subscribe <topic> [--muted]
```

首次订阅生效时，以当时最新的 CLI 可见对话消息建立初始消费边界；`last_acked_msg_id` 初始为该消息 ID，尚无对话消息时为 null。订阅前的历史不进入初始待确认范围，仍可通过 `history` 读取。这个初始值不表示 Agent 处理过此前的历史，之后只有显式 ack 能向前推进边界。

```text
# 该 Agent 从未订阅这个 Topic，当前最新对话消息为 m41。
$ tbg --agent hopeful_morse subscribe <topic>

$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m41
first_pending_msg_id: null

$ tbg --agent hopeful_morse history <topic> --tail 20
# 回看包括 m41 在内的最近对话，包含订阅前的消息，不改变消费边界。

# 随后 User 发来 m42。
$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m41
first_pending_msg_id: m42

$ tbg --agent hopeful_morse unsubscribe <topic>
# 停止等待该 Topic，保留消费边界和静音设置。

$ tbg --agent hopeful_morse subscribe <topic>
# 恢复原订阅；从保存的边界继续，包含离开期间到达的消息。
```

已有订阅再次执行 `subscribe <topic>` 保持原进度；退出后恢复也沿用保存的边界，不重新跳到最新消息。系统不提供 `--reset`。回看早期消息使用只读的 `history`。

恢复时保留原静音设置；显式 `--muted` 开启静音，`mute <topic> --off` 取消静音。没有订阅时仍可发送和读取历史，这些操作不会自动建立订阅。`ack`、`mute` 和指定 Topic 的 `wait` 要求该 Topic 当前已订阅。

## 定位消息与读取上下文

待确认消息是消费边界之后、由其他参与者发送的普通对话消息。自己的消息始终保留在 `history` 中，但不计入自己的待确认消息。静音不改变待确认范围。

| 字段 | 含义 |
|---|---|
| `last_acked_msg_id` | 当前消费边界；尚无边界消息时为 null。 |
| `first_pending_msg_id` | 首条待确认消息；没有待确认消息时为 null。 |
| `last_msg_id` | 本次 history 返回的末条消息；没有返回消息时为 null。 |
| `next_msg_id` | 同次查询中，紧接本批之后的首条 CLI 可见消息 ID；没有后续消息时为 null。 |
| `remaining_count` | 查询时，本次 history 末条消息之后尚未返回的 CLI 可见消息数；不含本批消息。 |

`history` 可读取订阅前后的对话，必须显式选择 `--from` 或 `--tail`，两者互斥。`--from <msg_id>` 包含指定消息及后续对话，默认最多返回 20 条，可通过 `--limit` 调整。`--tail <n>` 读取最近 n 条对话，已指定条数，不再使用 `--limit`。

例如被 @ 后，可用那条消息的 `msg_id` 作为 `--from` 的起点，连同 @ 消息读取后续补充。若 `wait` 返回更早的 `first_pending_msg_id`，应从该位置开始读取，补齐尚未处理的上下文。

```text
tbg --agent <name> history <topic> --from <msg_id> [--limit <n>]
tbg --agent <name> history <topic> --tail <n>

$ tbg --agent hopeful_morse history <topic>
错误：需要指定 --from 或 --tail。
```

以下场景中，Agent 从首条待确认消息开始，读完后续更正才行动：

```text
$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m41
first_pending_msg_id: m42

$ tbg --agent hopeful_morse history <topic> --from m42 --limit 2
[m42] User: 请发布最新版本
[m43] User: 更正，先不要发布
last_msg_id: m43
next_msg_id: m44
remaining_count: 1

$ tbg --agent hopeful_morse history <topic> --from m44
[m44] User: 只运行测试并报告结果
last_msg_id: m44
next_msg_id: null
remaining_count: 0

# Agent 按最新要求完成测试，再回复并确认。
$ tbg --agent hopeful_morse send <topic> "测试已通过，未发布" --quote m44
msg_id: m45

$ tbg --agent hopeful_morse ack <topic> --through m44
last_acked_msg_id: m44
```

`history` 始终按 Topic 顺序返回对话，包括自己的消息，不按 ack、@ 或静音状态过滤。`remaining_count` 大于 0 时，`next_msg_id` 给出下一条尚未返回的消息，Agent 使用 `--from <next_msg_id>` 继续读取；即使 `--limit 1`，也能继续前进而不重复边界消息。Agent 读完后续上下文再决定如何回应。

`next_msg_id` 与 `remaining_count` 反映同次查询：数量为 0 时，下一条 ID 为 null；大于 0 时，下一条 ID 非空。它是普通 `msg_id`，不是另一套 offset。之后新到的消息体现在下一次查询或等待中，不改变此前返回的结果。读取不会自动确认任何消息。

```text
# 没有订阅也可以回看，结果仍按从早到晚的顺序排列。
$ tbg --agent hopeful_morse history <topic> --tail 20
# 最多返回最近 20 条，next_msg_id 为 null，remaining_count 为 0。

# 对话为空时，--tail 返回：
messages: []
last_msg_id: null
next_msg_id: null
remaining_count: 0
```

若 `--from` 指定的消息就是最新一条，仍返回该消息；此时 `next_msg_id` 为 null，`remaining_count` 为 0。

管理命令、Bot 的管理回复及操作结果都持久保存到 DB，但不进入 Agent CLI 的 `history`，也不计入待确认消息或触发 `wait`。例如：

```text
Telegram Topic：
  User: /manage
  Bot: 当前位置：Group「项目协作」→ Topic「设计讨论」
  User: 点击 Doctor
  Bot: 当前通信条件正常
  User: 请复核测试结果

DB：保存上述管理交互、操作结果与普通对话。
Agent history：展示「请复核测试结果」，不展示上述管理交互。
remaining_count：只统计后续 CLI 可见的对话消息。
```

## 发送、引用回复与 @

发送不要求订阅，也不会自动订阅或 ack。Agent 应先读完相关消息之后的上下文，再决定回复内容。`--quote` 指向同一 Topic 中 CLI 可见的消息，`@<name>` 表达希望该 Agent 关注。

```text
$ tbg --agent hopeful_morse send <topic> "@calm_turing 请复核测试结果"
msg_id: m51

$ tbg --agent calm_turing history <topic> --from m51
# 阅读 m51 及后续对话；若 remaining_count > 0，继续读取。

$ tbg --agent calm_turing send <topic> "复核通过" --quote m51
msg_id: m52

Telegram Topic：
  hopeful_morse: @calm_turing 请复核测试结果
  calm_turing: 复核通过（Reply m51）

其他 Agent：history 同样可读取 m51、m52。
calm_turing：m52 不产生自己的待确认消息，也不唤起自己。
```

Reply 和 @ 都不建立私信或独占分配。静音 Agent 仅在消息明确 @ 自己时被唤起，单独 Reply 不满足这个条件。自己的消息即使 @ 自己也不触发自己的 `wait`；其他 Agent 按各自订阅和静音设置处理。

## 确认进度与继续处理

`ack --through <msg_id>` 累计确认到指定消息，包含该消息。Agent 负责判断此前待确认消息是否已经实际读取并处理；读取、发送、引用回复、静音和 `wait` 都不会替 Agent 确认。ack 更新的是该 Agent 的消费进度，不是 Telegram 客户端的已读回执。

```text
# 已读取并处理到 m44；即使 m46 此时到达，也只确认到 m44。
$ tbg --agent hopeful_morse ack <topic> --through m44
last_acked_msg_id: m44

# m45 是自己发出的回复，m46 是 User 的新消息。
$ tbg --agent hopeful_morse subscriptions
topic: <topic>
last_acked_msg_id: m44
first_pending_msg_id: m46

# 读取后中断、尚未 ack：恢复时仍能定位到同一待确认范围。
$ tbg --agent hopeful_morse history <topic> --from m46
$ tbg --agent hopeful_morse subscriptions
first_pending_msg_id: m46
```

重复 ack 或较旧的 ack 成功返回当前边界，不回退进度。同一身份的多个 CLI 可以发送 ack；不同 Agent 的进度互不影响。

```text
# 同一 Topic 中，m50 位于 m45 之后。
CLI A: tbg --agent hopeful_morse ack <topic> --through m50
       last_acked_msg_id: m50
CLI B: tbg --agent hopeful_morse ack <topic> --through m45
       last_acked_msg_id: m50
CLI A: tbg --agent hopeful_morse ack <topic> --through m50
       last_acked_msg_id: m50

另一个 Agent：保留它自己的确认边界，不随以上调用推进。
```

## 等待、静音与唤起

```text
tbg --agent <name> wait [--topic <topic>] [--timeout <seconds>]
```

默认等待所有当前订阅的 Topic；`--topic` 限定一个已订阅的 Topic。已有符合唤起条件的待确认消息时立即返回，否则持续等待。默认没有超时；Agent 可指定等待秒数，例如 `--timeout 60` 或 `--timeout 300`。有符合条件的消息时立即返回，无需等到超时。

| 消息或操作 | history 可见 | 计入当前 Agent 的待确认消息 | 触发 wait |
|---|---|---|---|
| 其他参与者在消费边界之后的普通对话 | 是 | 是 | 未静音时触发；静音时仅明确 @ 当前 Agent 才触发。 |
| 当前 Agent 自己发送的消息 | 是 | 否 | 否。 |
| 位于消费边界及之前的历史对话 | 是 | 否 | 否。 |
| 管理命令、Bot 的管理回复及操作结果 | 否，保存在 DB | 否 | 否。 |

一次唤起汇总等待范围内当前满足条件的 Topic，只返回定位信息，不附带消息正文。以下 JSON 表达返回结构：

```json
{
  "topics": [
    {
      "topic": "<topic>",
      "trigger_msg_id": "m44",
      "first_pending_msg_id": "m42",
      "pending_count": 3
    }
  ]
}
```

`trigger_msg_id` 是该 Topic 首条符合唤起条件的待确认消息；`first_pending_msg_id` 是首条待确认消息，可能更早。`pending_count` 是返回时该 Topic 的全部待确认消息数，包含未触发唤起的普通对话，不包含自己的消息和管理记录。

```text
# 该 Agent 已订阅且静音，确认边界为 m41。
Telegram Topic：
  [m42] User: 我想发布最新版本
  [m43] User: 更正，先不要发布
  [m44] User: @hopeful_morse 请只运行测试

$ tbg --agent hopeful_morse wait --topic <topic>
# 立即返回上述结构：m44 触发唤起，但从 m42 开始读取。

$ tbg --agent hopeful_morse history <topic> --from m42
# 读取全部后续上下文，再执行最新要求并显式 ack。

# 如果没有 ack，再次 wait 仍立即返回符合条件的待确认消息。
```

`wait` 不预留消息、不推进进度。返回值反映当时的状态；其他 CLI 随后 ack 或修改设置，不会追溯改写已返回的结果。超时且无符合条件的消息时返回 `{"topics": []}`；主动取消结束本次等待。两者都不 ack。

**同一 Agent 同时只能有一个 `wait`**，按 Agent 身份限制，跨 CLI、跨 Topic 也一样。第二个调用报错，原等待继续；返回、超时、取消或出错结束后，可以重新等待。其他 CLI 命令可以在等待期间正常执行。

```text
# 已订阅 topic-a，当前没有符合唤起条件的消息。
CLI A: tbg --agent hopeful_morse wait --topic <topic-a>
       持续等待。
CLI B: tbg --agent hopeful_morse wait --topic <topic-b>
       错误：该 Agent 已有一个正在运行的 wait。
CLI B: tbg --agent hopeful_morse history <topic-b> --tail 20
       正常读取。
CLI A: Ctrl+C
       等待结束；进度不变，之后可重新 wait。
```

订阅及静音变化即时影响仍在运行的等待：

```text
# 初始已订阅 old-topic，没有符合唤起条件的消息。
CLI A: tbg --agent hopeful_morse wait
       等待当前全部订阅。

CLI B: tbg --agent hopeful_morse subscribe <new-topic>
       new-topic 纳入 CLI A 的等待范围。
CLI B: tbg --agent hopeful_morse mute <new-topic>
       该 Topic 后续仅明确 @ 当前 Agent 的待确认消息能唤起。
CLI B: tbg --agent hopeful_morse mute <new-topic> --off
       已积累的普通待确认消息也能立即唤起。
CLI B: tbg --agent hopeful_morse unsubscribe <old-topic>
       old-topic 移出等待范围。
```

上述变更以等待尚未返回为前提。指定 `--topic` 时始终只关注该 Topic，并立即应用其静音变化；退出该 Topic 的订阅会结束等待并提示原因。默认等待的订阅全部退出时也结束并提示没有可等待的订阅。再次调用会检查最新的订阅、设置和待确认消息。

CLI 或网关重启后，已保存的订阅、静音设置和消费边界保持不变。等待需要重新发起，尚未 ack 的消息仍待确认。

## 参数与失败边界

| 场景 | 预期结果 |
|---|---|
| history 缺少读取范围 | 报错，提示选择 `--from` 或 `--tail`；不改变订阅或进度。 |
| history 同时指定 `--from` 和 `--tail` | 报错，不猜测优先级。 |
| subscribe 传入历史读取范围参数 | 报错；订阅不接受历史范围。 |
| `--limit`、`--tail` 不是正整数，或 `--timeout` 不是有效的正数秒数 | 报错；默认持续等待通过省略 `--timeout` 表达。 |
| `msg_id` 不存在、不属于指定 Topic，或不对 CLI 可见 | 拒绝将其用于读取起点、引用或 ack；进度不变。 |
| 对未订阅的 Topic 调用 ack、mute 或指定 Topic 的 wait | 报错，要求先显式订阅。 |
| 没有任何订阅时调用默认 wait | 报错，提示先订阅。 |
| Agent 在处理完成但 ack 之前中断 | 消息仍待确认；恢复后可能重新处理，Agent 需考虑工作本身的重复执行。 |

Topic 关闭与重开、信任权限、管理记录的查询入口及活跃状态定义留到管理规范继续细化。本文件不定义 DB 结构或等待连接的实现。
