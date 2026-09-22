# 操作能力总览

状态：规划中，接口草案，尚未实现。[English](README.md)

Agent 使用 `tbg` CLI，User 使用 Telegram Bot。本文汇总两个入口的能力；通信场景和错误边界见[通信规范](communication.zh-CN.md)，管理状态与交互将在管理规范中继续收敛。

## CLI：tbg --help

`tbg` 专供 Agent 使用。Agent 自助注册，获得自动生成的唯一名称；后续携带该名称操作。Agent 的内部 ID 不作为使用者需要管理的标识。同一名称在不同 CLI 中共享订阅、静音设置和 ack 进度，不同 Agent 的消费进度各自独立。

```text
tbg — Telegram Bot Gateway

USAGE
  tbg agent register
  tbg --agent <name> <command>

IDENTITY
  agent register                         注册并返回自动生成的唯一名称
  whoami                                 查看当前 Agent 身份
  agent list                             查看 Agent 及参与状态

TOPIC
  group list                             查看已接入的 Group
  topic list [--group <group>]            查看 Topic 及状态
  topic show <topic>                      查看 Topic 和订阅者
  topic create --group <group> --name <name>
                                         创建 Topic
  topic close <topic>                    关闭自己创建的 Topic
  topic reopen <topic>                   重新打开自己创建的 Topic

COMMUNICATION
  send <topic> "<content>" [--quote <msg_id>]
                                         发送消息；--quote 引用回复，正文支持 @
  history <topic> --from <msg_id> [--limit <n>]
                                         从指定消息开始读取，包含该消息
  history <topic> --tail <n>              读取最近 n 条消息
  ack <topic> --through <msg_id>          累计确认到该消息，包含该消息
  wait [--topic <topic>] [--timeout <seconds>]
                                         默认持续等待所有当前订阅，每个 Agent 只允许一个 wait

SUBSCRIPTION
  subscribe <topic> [--muted]             订阅新消息，或恢复已有订阅及原进度
  subscriptions                         查看订阅、已确认消息 ID 和首条待确认消息 ID
  mute <topic> [--off]                   开启静音；--off 取消静音
  unsubscribe <topic>                    停止订阅，保留进度
```

注册示例：

```text
$ tbg agent register
hopeful_morse

$ tbg --agent hopeful_morse group list
$ tbg --agent hopeful_morse topic list --group <group>
```

新 Agent 没有默认订阅。subscribe 管理订阅，首次从订阅生效后的新消息开始消费，新订阅默认 unmute。发送和 history 都不要求订阅，也不会自动建立订阅；history 可以读取订阅前的对话。default topic 暂时预留。

Topic 是共享对话空间。Reply 保留回应关系，@ 表达希望谁关注，两者都不改变消息对订阅者的可见性。静音只影响 wait 的提醒：静音时，仅明确 @ 当前 Agent 的消息触发 wait，其他消息仍可主动读取。

wait 默认持续等待，有符合唤起条件的待确认消息时立即返回，包括调用前已到达的消息。可通过 `--timeout <seconds>` 指定秒数，超时返回空的 topics 列表。每个 Agent 同时只能有一个 wait，第二个调用报错；订阅和静音变化即时影响当前等待，指定 `--topic` 时始终限定在该 Topic。wait 返回满足条件的 Topic 及 `trigger_msg_id`、`first_pending_msg_id`、`pending_count`，正文通过 history 读取；未 ack 的消息仍可再次触发 wait。

每条消息对外使用一个稳定的 `msg_id`，由 send 返回，并在 history 中展示。引用、读取起点和 ack 使用同一消息标识，不再要求 Agent 管理另一套 position 或 offset。history 的 `--from` 包含指定消息及后续对话，可用于定位 @ 所在的消息；ack 的 `--through` 包含指定消息；`--quote` 指向被回复的消息。

subscriptions 为每个订阅返回 `last_acked_msg_id` 和 `first_pending_msg_id`，后者是消费边界之后、由其他参与者发送的首条待确认消息的位置，没有待确认消息时为 null。Agent 根据这个位置调用 history 读取该消息及后续对话；history 按 Topic 中的消息顺序返回，不按是否已确认、是否被 @ 或是否静音过滤内容。自己的消息保留在 history 中，不计入自己的待确认消息，也不唤起自己。

管理命令、Bot 的管理回复和操作结果持久保存到 DB，但不出现在 Agent CLI 的 history 中，也不计入待确认消息或唤起 Agent。

history 必须显式选择 `--from` 或 `--tail`，两者互斥。`--from` 默认最多返回 20 条，可用 `--limit` 调整；`--tail <n>` 读取最近 n 条消息。`last_msg_id` 是本次返回的末条消息 ID，`remaining_count` 是同次查询中其后尚未返回的 CLI 可见消息数量，不包含本批消息或管理记录。`next_msg_id` 指向下一条尚未返回的消息，数量为 0 时为 null。Agent 使用 `--from <next_msg_id>` 继续分页，读完后续上下文再回应；之后新到的消息在下次查询或等待中体现。

以下示例假设 Agent 已订阅该 Topic。User 后续更正了要求，Agent 读完两批消息后才回复：

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

$ tbg --agent hopeful_morse history <topic> --from m44 --limit 20
[m44] User: 只运行测试并报告结果
last_msg_id: m44
next_msg_id: null
remaining_count: 0

# Agent 按最新要求完成测试，再回复并确认这批消息。
$ tbg --agent hopeful_morse send <topic> "测试已通过，未发布" --quote m44
msg_id: m45

$ tbg --agent hopeful_morse ack <topic> --through m44
```

发送、引用回复、读取历史、静音和 wait 都不会自动确认消费，已处理进度仍由 Agent 显式 ack。Agent 确认到实际读取并处理完的末条消息，后续由其他参与者发来的普通对话仍待确认。重复或较旧的 ack 成功返回当前进度，进度不会后退。history 的读取范围只影响本次调用，不会重置订阅进度。

subscribe 不提供历史范围参数。已有订阅再次调用或退出后恢复时保留原进度，不重新跳到最新消息；不提供 `--reset`。首次订阅的消费边界及恢复场景见[通信规范](communication.zh-CN.md)。

## Telegram Bot：/help

User 在 Topic 中发言、Reply 或 @ Agent。查询与管理统一从 `/manage` 进入，通过按钮操作；菜单根据当前位置和 User 权限展示可用能力。

```text
/help       查看使用帮助
/whoami     查看自己的 Telegram user_id 和身份
/manage     打开当前位置的查询与管理菜单
```

菜单始终提示当前位置与操作范围。例如在 Topic 内打开：

```text
当前位置：Group「项目协作」→ Topic「设计讨论」
操作范围：当前 Topic
```

管理员在与 Bot 的私聊中可以进入全局管理。以下是完整能力目录；Topic 内的菜单按当前范围提供相关操作。

```text
/manage
├─ 状态总览
├─ Agent
│  └─ [全部] [活跃] [等待中]
├─ Group
├─ Topic
│  └─ 查看、关闭、重新打开
├─ User 信任（管理员）
│  └─ 查看、授予、撤销
├─ Doctor
│  └─ 检查通信条件并给出处理建议
└─ 个人设置
   └─ 语言
      └─ [跟随 Telegram] [简体中文] [English]
```

子页面提供返回按钮，筛选通过按钮更新列表。User 分为管理员和普通可信 User：管理员管理信任名单、Group 接入与全部 Topic；普通可信 User 参与对话、查看相关状态。

首次使用时，User 通过 `/whoami` 获取自己的 user_id，并将它写入配置文件的管理员名单。管理员随后可以在菜单中直接授予或撤销普通 User 的信任。管理员将 Bot 拉进 Group 后，网关自动登记，Doctor 展示可用能力及缺少的权限。

语言设置按 User 保存，默认跟随其 Telegram 语言，也可手动选择。语言字段缺失时沿用上次记录，无法选择支持的语言时回退到 English。菜单使用打开者的语言；本地化覆盖菜单、按钮、提示和诊断文字，名称与对话内容保留原文。语言判定参考 [Telegram 的语言支持约定](https://core.telegram.org/bots/features#language-support)。

## 详细规范

| 文件 | 职责 |
|---|---|
| [communication.zh-CN.md](communication.zh-CN.md) | Agent CLI 与 Telegram User 的通信场景：订阅、发送、读取、ack、wait、Reply/@、暂离与恢复。 |
| `operations.zh-CN.md`（待编写） | 注册与身份、Bot 配置、User 信任、Group/Topic 管理、菜单交互、Doctor、状态及语言设置。 |

本轮先收敛 CLI 操作与 Telegram 交互。CLI → Gateway → Telegram 的职责、传输方式和状态存储留给后续架构文档。历史检索与 Checkpoint 暂不设计。
