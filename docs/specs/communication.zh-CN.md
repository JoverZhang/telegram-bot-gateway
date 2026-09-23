# 通信规范

状态：规划中，行为规范草案，尚未实现。[English](communication.md)

本文说明 Agent 如何订阅 Topic、读取对话、回复并确认处理进度。命令入口与输出约定见[操作能力总览](README.zh-CN.md)；以下场景独立，消息 ID 为示例，未列出的 CLI 结果已省略。

Topic 中的普通对话对参与者共享。每个 Agent 的订阅和确认进度独立，同一 Agent 名称下的 CLI 共享状态。`msg_id` 是引用、读取游标和 ack 共用的稳定标识；消息先后由 Topic 中的顺序确定，不能通过 ID 相减计算数量。

读取游标、`--quote` 和 ack 使用的 `msg_id` 必须存在于指定 Topic 且对 CLI 可见，否则拒绝操作，进度不变。

## 订阅、退出与恢复

首次 `subscribe` 从订阅生效后的新消息开始消费，默认不静音；`--muted` 可在订阅时开启静音。

首次订阅以当时最新的 CLI 可见对话消息初始化 `last_acked_msg_id`，没有消息时为 null。这个消费边界只决定起点，不表示此前历史已处理；订阅前的历史仍可通过 `history` 读取。

```text
# 首次订阅，当前最新对话消息为 m41。
$ tbg --agent hopeful_morse subscribe <topic>
$ tbg --agent hopeful_morse unsubscribe <topic>
# 退出期间 User 发来 m42。
$ tbg --agent hopeful_morse subscribe <topic>
$ tbg --agent hopeful_morse unread <topic> | jq .
{
  "messages": [
    {
      "msg_id": "m42",
      "sent_at": "2026-09-22T09:10:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "请检查测试结果"
    }
  ],
  "next_cursor": "m42",
  "remaining_count": 0
}
```

重复订阅或退出后恢复均保留消费边界和静音设置。显式 `--muted` 开启静音，`mute <topic> --off` 取消静音。

send 和 history 不要求订阅，也不会自动建立订阅。unread、ack、mute 和指定 Topic 的 wait 要求当前已订阅该 Topic，否则报错。

## 定位消息与读取上下文

`unread` 用于接续处理，`history` 用于回看上下文。两者都只读，默认最多返回 20 条，可通过 `--limit` 调整。

两者返回 `messages` 数组，每条消息包含以下字段：

| 字段 | 含义 |
|---|---|
| `msg_id` | 稳定的消息标识，供引用、读取游标和 ack 使用。 |
| `sent_at` | 消息发送时间，使用带日期的 UTC 时间，如 `2026-09-22T09:10:00Z`；消息排序仍以 Topic 中的顺序为准。 |
| `sender` | Agent 使用 `{"type":"agent","name":"calm_turing"}`；User 使用 `{"type":"user","user_id":12345678}`。 |
| `content` | 完整消息正文，保留换行。 |
| `quote_msg_id` | 被回复的消息 ID；没有引用时省略。 |

| 命令 | 省略 cursor 时的起点 | 读取及输出顺序 |
|---|---|---|
| `unread` | 当前消费边界之后的首条 CLI 可见对话消息；边界为 null 时从首条开始。 | 从旧到新。 |
| `history` | 当前最新的 CLI 可见对话消息，可读到订阅前的历史。 | 从新到旧。 |

`--cursor <msg_id>` 表示上次读到的位置，排除该消息：unread 读取更晚的消息，history 读取更早的消息。将返回的 `next_cursor` 传给同一命令即可继续分页，无需提前 ack。显式 cursor 不修改消费边界，也不因其他 CLI 的 ack 改变起点；省略 cursor 时重新使用默认起点。

subscriptions 只查看保存的订阅进度，不改变下一次 unread 的起点。未 ack 时再次执行不带 cursor 的 unread，会重新返回尚未确认的对话；连续读取使用 cursor，中断后从 ack 恢复则允许重读。同一 Agent 的多个 CLI 也可能读到相同消息，读取不独占分配消息。

待确认消息是消费边界之后、由其他参与者发送的普通对话。unread 和 history 返回读取范围内的连续对话，包括自己的发言，不按 @ 或静音过滤。自己的消息不计入待确认消息，因此即使没有待确认消息，unread 也可能返回自己的发言。

| 字段 | 含义 |
|---|---|
| `last_acked_msg_id` | subscriptions 和 ack 返回的当前消费边界；尚无边界消息时为 null。 |
| `next_cursor` | 本批按输出顺序返回的末条消息的 `msg_id`。空批次保留传入的 cursor；未传 cursor 的 unread 保留消费边界，history 面对空 Topic 时为 null。尚无任何位置时为 null。 |
| `remaining_count` | 查询时，沿当前读取方向尚未返回的 CLI 可见消息数；unread 统计更晚的消息，history 统计更早的消息，均不含本批。 |

Agent 应先读完后续对话再行动和回复，避免遗漏更正：

```text
# 已订阅，消费边界为 m41；User 随后发来 m42、m43、m44。
$ tbg --agent hopeful_morse unread <topic> --limit 2 | jq .
{
  "messages": [
    {
      "msg_id": "m42",
      "sent_at": "2026-09-22T09:10:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "请发布最新版本"
    },
    {
      "msg_id": "m43",
      "sent_at": "2026-09-22T09:11:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "更正，先不要发布"
    }
  ],
  "next_cursor": "m43",
  "remaining_count": 1
}

$ tbg --agent hopeful_morse unread <topic> --cursor m43 | jq .
{
  "messages": [
    {
      "msg_id": "m44",
      "sent_at": "2026-09-22T09:12:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "只运行测试并报告结果"
    }
  ],
  "next_cursor": "m44",
  "remaining_count": 0
}

# 按最新要求完成测试，再回复并确认。
$ tbg --agent hopeful_morse send <topic> "测试已通过，未发布" --quote m44 | jq .
{
  "msg_id": "m45"
}
$ tbg --agent hopeful_morse ack <topic> --through m44 | jq .
{
  "last_acked_msg_id": "m44"
}
```

需要更早的背景时，通过 history 向旧消息回看：

```text
# 独立场景：Topic 仅有 m41、m42、m43、m44 四条对话消息。
$ tbg --agent hopeful_morse history <topic> --limit 2 | jq .
{
  "messages": [
    {
      "msg_id": "m44",
      "sent_at": "2026-09-22T09:12:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "只运行测试并报告结果"
    },
    {
      "msg_id": "m43",
      "sent_at": "2026-09-22T09:11:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "更正，先不要发布"
    }
  ],
  "next_cursor": "m43",
  "remaining_count": 2
}

$ tbg --agent hopeful_morse history <topic> --cursor m43 --limit 2 | jq .
{
  "messages": [
    {
      "msg_id": "m42",
      "sent_at": "2026-09-22T09:10:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "请发布最新版本"
    },
    {
      "msg_id": "m41",
      "sent_at": "2026-09-22T09:09:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "帮我检查项目"
    }
  ],
  "next_cursor": "m41",
  "remaining_count": 0
}
```

`remaining_count` 为 0 仅表示查询时该方向没有更多消息，不清空 cursor。每次调用重新查询，不固定整个分页过程的快照。保留 unread 的 cursor 可以继续读取后来到达的消息；history 反向续读不会返回更晚到达的消息，查看最新对话需省略 cursor 重新调用。

```text
# 已读到 m44，尚未 ack，也没有新消息。
$ tbg --agent hopeful_morse unread <topic> --cursor m44 | jq .
{
  "messages": [],
  "next_cursor": "m44",
  "remaining_count": 0
}

# User 随后发来 m45；使用原 cursor 即可继续，不重读 m44。
$ tbg --agent hopeful_morse unread <topic> --cursor m44 | jq .
{
  "messages": [
    {
      "msg_id": "m45",
      "sent_at": "2026-09-22T09:13:00Z",
      "sender": {
        "type": "user",
        "user_id": 12345678
      },
      "content": "请附上测试报告"
    }
  ],
  "next_cursor": "m45",
  "remaining_count": 0
}
```

空 Topic 且没有已知读取位置时返回：

```json
{
  "messages": [],
  "next_cursor": null,
  "remaining_count": 0
}
```

管理交互（命令、菜单点击、Bot 的管理回复及操作结果）持久保存到 DB，但不进入 unread 或 history，也不计入待确认消息或 remaining_count，不触发 wait。

## 发送、引用回复与 @

send 的 `--quote` 指向被回复的消息，正文中的 `@<name>` 表达希望该 Agent 关注。

```text
# calm_turing 已订阅，以下消息位于其消费边界之后。
$ tbg --agent hopeful_morse send <topic> "@calm_turing 请复核测试结果" | jq .
{
  "msg_id": "m51"
}

$ tbg --agent calm_turing unread <topic>
# 读完后续对话，再回复。
$ tbg --agent calm_turing send <topic> "复核通过" --quote m51 | jq .
{
  "msg_id": "m52"
}

Telegram Topic：
  hopeful_morse: @calm_turing 请复核测试结果
  calm_turing: 复核通过（Reply m51）
```

Reply 和 @ 都不建立私信或独占分配，其他 Agent 仍可读取同一对话。

## 确认进度与继续处理

`ack --through <msg_id>` 累计确认到指定消息，包含该消息。Agent 负责判断此前待确认消息是否已实际读取并处理；只有显式 ack 推进消费边界，读取、发送、引用回复、静音和 wait 均不代为确认。`next_cursor` 不代表已处理进度，ack 也不代表 Telegram 客户端的已读回执。

```text
# 已处理到 m44；m45 是自己的回复，m46 是 User 的新消息。
$ tbg --agent hopeful_morse ack <topic> --through m44 | jq .
{
  "last_acked_msg_id": "m44"
}

$ tbg --agent hopeful_morse unread <topic>
# 从 m45 开始，包含自己的回复和 User 的 m46。
# 读取后中断、尚未 ack，消费边界不变。
$ tbg --agent hopeful_morse subscriptions | jq .
{
  "subscriptions": [
    {
      "topic": "<topic>",
      "last_acked_msg_id": "m44"
    }
  ]
}
```

重复或较旧的 ack 成功返回当前边界，不回退进度。同一身份的多个 CLI 可以发送 ack：

```text
# 同一 Topic 中，m50 位于 m45 之后。
CLI A: tbg --agent hopeful_morse ack <topic> --through m50 | jq .
       {
         "last_acked_msg_id": "m50"
       }
CLI B: tbg --agent hopeful_morse ack <topic> --through m45 | jq .
       {
         "last_acked_msg_id": "m50"
       }
```

处理完成但 ack 之前中断时，消息仍待确认，恢复后可能重新处理；Agent 需考虑工作本身的重复执行。

## 等待、静音与唤起

wait 默认持续等待所有当前订阅的 Topic，`--topic` 限定一个已订阅的 Topic。已有符合条件的待确认消息时立即返回，否则继续等待；可通过 `--timeout 300` 这样的参数指定超时秒数。没有任何订阅时，默认 wait 报错。

未静音时，任意待确认消息都可唤起；静音时仅明确 @ 自己的消息可唤起，单独 Reply 不满足条件。自己的消息即使 @ 自己也不唤起自己。

一次唤起汇总等待范围内当前满足条件的 Topic，只返回定位信息，不附带正文：

```json
{
  "topics": [
    {
      "topic": "<topic>",
      "trigger_msg_id": "m44",
      "pending_count": 3
    }
  ]
}
```

`trigger_msg_id` 是该 Topic 首条符合唤起条件的待确认消息；它说明唤起原因，unread 仍从消费边界之后读取完整对话。`pending_count` 统计该 Topic 的全部待确认消息，包含未触发唤起的对话，不包含自己的消息和管理记录。

```text
# 已订阅且静音，消费边界为 m41。
Telegram Topic：
  [m42] User: 我想发布最新版本
  [m43] User: 更正，先不要发布
  [m44] User: @hopeful_morse 请只运行测试

$ tbg --agent hopeful_morse wait --topic <topic> | jq .
# 返回上述结构：m44 触发唤起，但从 m42 开始读取。
$ tbg --agent hopeful_morse unread <topic>
# 读完后续对话，再执行最新要求并显式 ack。
# 未 ack 时，再次 wait 仍立即返回符合条件的待确认消息。
```

wait 不预留消息。返回值反映当时状态，其他 CLI 随后的 ack 或设置变更不会改写已返回的结果。主动取消结束本次等待。超时且无符合条件的消息时返回：

```json
{
  "topics": []
}
```

**同一 Agent 同时只能有一个 wait**，跨 CLI、跨 Topic 也一样。第二个调用报错，原等待继续；返回、超时、取消或出错结束后可重新等待，其他命令在等待期间仍可执行。

```text
# 已订阅 topic-a 和 topic-b，当前没有符合唤起条件的消息。
CLI A: tbg --agent hopeful_morse wait --topic <topic-a>
       持续等待。
CLI B: tbg --agent hopeful_morse wait --topic <topic-b>
       错误：该 Agent 已有一个正在运行的 wait。
CLI B: tbg --agent hopeful_morse history <topic-b>
       正常读取。
CLI A: Ctrl+C
       等待结束；之后可重新 wait。
```

订阅及静音变化即时影响尚未返回的 wait：

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

指定 `--topic` 时始终只关注该 Topic，并立即应用其静音变化；退出该订阅会结束等待并提示原因。默认等待的订阅全部退出时，也结束并提示没有可等待的订阅。

CLI 或网关重启后，订阅、静音设置和消费边界保持不变，等待需要重新发起。

Topic 关闭与重开、信任权限、管理记录查询及活跃状态定义留到管理规范继续细化；DB 结构和传输实现留给后续架构文档。
