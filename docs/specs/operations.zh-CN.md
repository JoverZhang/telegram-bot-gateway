# 管理规范

状态：规划中，讨论草案，尚未实现。[English](operations.md)

本文描述如何建立通信环境、管理参与者，以及发现通信异常。命令入口见[操作能力总览](README.zh-CN.md)，消息消费规则见[通信规范](communication.zh-CN.md)。标为“建议（待确认）”的内容供讨论；新增配置字段和返回字段也属于草案。CLI 实际输出紧凑 JSON，文档示例通过 `jq .` 格式化展示。

## 首次启动与管理员身份

Gateway 配置 Bot token 和初始管理员的 Telegram user_id；CLI 需要 Gateway 地址，并通过 `--agent` 选择身份。配置文件的格式、路径及加载方式待确认。

User 的管理员身份来自 Gateway 配置；Bot 的群管理员权限由 Telegram 群设置授予。

```yaml
# Gateway 配置示意，字段名与格式待确认。
telegram:
  bot_token: "<bot_token>"
admins:
  - 12345678
```

User 通过私聊 Bot 的 `/whoami` 获取自己的 user_id，再写入管理员名单。

建议（待确认）：尚未配置管理员时，Bot 允许 `/help` 和 `/whoami`，提示完成初始化；管理和 Group 接入暂不可用。

```text
# 建议的初始化流程：Bot 已连接 Telegram，尚未配置管理员。
User → Bot：/whoami
Bot：
  user_id: 12345678
  身份：未信任
  尚未配置管理员，请将此 user_id 写入 Gateway 的管理员名单。

# 管理员配置生效后。
User → Bot：/whoami
Bot：
  user_id: 12345678
  身份：管理员

User → Bot：/manage
Bot：
  当前位置：与 Bot 的私聊
  操作范围：全局
  [状态总览] [Agent] [Group] [Topic]
  [User 信任] [Doctor] [个人设置]
```

## Agent 身份与 CLI 状态

Agent 通过 `agent register [--name <name>]` 注册。仅省略 `--name` 时自动生成名称。显式传入的值按原样校验；空字符串、其他非法名称或名称冲突均报错，不自动修剪、替换或改名。注册失败不改变已有 Agent 的状态。

注册成功后使用返回的唯一名称；同一名称对应同一份订阅、静音设置和 ack 进度，由 Gateway 保存，CLI 不另存一份消费边界。

```text
# 指定名称。
$ tbg agent register --name hopeful_morse | jq .
{
  "name": "hopeful_morse"
}

# 仅省略 --name 时自动生成。
$ tbg agent register | jq .
{
  "name": "calm_turing"
}

$ tbg agent register --name ""
# 注册失败：名称不能为空，不自动生成名称。

$ tbg agent register --name hopeful_morse
# 注册失败：名称已被使用，已有 Agent 的状态不变。

$ tbg --agent hopeful_morse whoami | jq .
{
  "name": "hopeful_morse"
}

# 同一身份的两个 CLI 共享状态；连接信息的配置方式待确认。
CLI A: tbg --agent hopeful_morse subscribe <topic>
CLI B: tbg --agent hopeful_morse subscriptions | jq .
       {
         "subscriptions": [
           {
             "topic": "<topic>",
             "last_acked_msg_id": "m41"
           }
         ]
       }
```

本例假设订阅时最新消息为 m41。需要独立消费进度的 Agent 分别注册；读取、确认及恢复规则沿用通信规范。

## User 信任

| 身份 | 已确认的能力 |
|---|---|
| 管理员 | 管理 User 信任、接入 Group、管理全部 Topic，并在私聊中查看全局状态。 |
| 普通可信 User | 参与对话，查看相关状态。 |
| 未信任 User | 通过 `/whoami` 获取自己的 user_id；普通发言的处理规则待确认。 |

管理员从菜单直接授予或撤销普通 User 的信任，以 user_id 指定对象。

```text
# 管理员与 Bot 私聊。
/manage → [User 信任] → [授予]
Bot：请输入 Telegram user_id。
管理员：87654321
Bot：已授予 87654321 信任。

/manage → [User 信任] → [87654321] → [撤销]
Bot：已撤销 87654321 的信任。
```

建议（待确认）：撤销后立即拒绝该 User 的后续受限操作，保留已经接收的历史。未信任 User 的普通消息是否保存、是否进入 Agent 的对话，以及撤销时正在执行的操作如何处理，仍需确定。

## 接入 Group 与管理 Topic

管理员将 Bot 加入 Group 后，Gateway 自动登记。Doctor 在该 Group 的菜单中展示通信条件及缺少的权限。

```text
# 管理员将 Bot 加入已启用 Topic 的 Group「项目协作」。
管理员在 Group 中输入：/manage
Bot：
  当前位置：Group「项目协作」
  操作范围：当前 Group
  [状态总览] [Agent] [Topic] [Doctor] [个人设置]

管理员点击：[Doctor]
# 查看权限与诊断建议，示例见下文。
```

Agent 可以自动创建 Topic，并关闭或重新打开自己创建的 Topic；管理员可通过菜单管理全部 Topic。以下返回字段为草案。

```text
$ tbg --agent hopeful_morse topic create --group <group> --name "设计讨论" | jq .
{
  "topic": "<topic>",
  "name": "设计讨论",
  "status": "active"
}

$ tbg --agent hopeful_morse topic close <topic> | jq .
{
  "topic": "<topic>",
  "status": "closed"
}

$ tbg --agent hopeful_morse topic reopen <topic>
# Topic 重新处于 active 状态。
```

建议（待确认）：Topic 使用 `active` 和 `closed` 表示开放与关闭；关闭后按下表处理。

| 操作或状态 | 关闭后的建议行为 |
|---|---|
| Agent 发送消息 | 拒绝发送，提示 Topic 已关闭。 |
| history、unread、ack | 继续允许，便于查看和处理已有对话。 |
| 已有订阅和消费边界 | 保留，重新打开时沿用。 |
| 新订阅、正在运行的 wait | 待确认。 |

由非管理员邀请 Bot、Bot 被移出 Group，以及直接在 Telegram 中改变 Topic 状态时的处理，待确认。

## 菜单范围与运行状态

`/manage` 跟随当前位置，页面始终显示操作范围。在 Topic 内查看 Agent 时展示当前 Topic 的订阅者；管理员在与 Bot 的私聊中查看全局。完整菜单目录见操作能力总览。

```text
# 在 Topic「设计讨论」中打开菜单。
/manage → [Agent]
当前位置：Group「项目协作」→ Topic「设计讨论」
操作范围：当前 Topic
[全部] [活跃] [等待中]

hopeful_morse   活跃，正在等待当前 Topic
calm_turing     活跃，未等待
[返回]
```

`waiting` 表示该 Agent 当前有一个尚未结束的 wait；同一 Agent 只能有一个 wait。建议（待确认）：`active` 表示最近 5 分钟内成功调用过 Gateway，或当前仍在 wait。等待中的 Agent 持续计入活跃；这两个筛选可以重叠。Group/Topic 范围内“等待中”是否只统计 wait 覆盖该范围的 Agent，待确认。

```text
# 管理员在私聊中查看全局；数量仅为示例。
/manage → [状态总览]
Bot：已连接 Telegram
Agent：共 4 个，活跃 2 个，其中等待中 1 个
Group：已接入 2 个
Topic：active 3 个，closed 1 个
[Agent] [Group] [Topic] [Doctor] [返回]

/manage → [Group]
项目协作
日常任务
[返回]

/manage → [Topic]
[active] [closed]
```

建议（待确认）：每次点击按钮都重新检查操作者权限；页面可以展示此前状态，操作结果以点击时的权限和状态为准。其他 User 点击已打开菜单时如何切换范围与语言，待确认。

## Doctor 与语言设置

Doctor 从 `/manage` 进入，说明检查范围、发现的问题及下一步操作。Group 中创建 Topic 需要 Bot 拥有管理员身份和 `can_manage_topics` 权限；普通群消息的可见性还与管理员身份及 Privacy Mode 有关。依据：[Topic 创建要求](https://core.telegram.org/bots/api#createforumtopic)、[群消息接收规则](https://core.telegram.org/bots/faq#what-messages-will-my-bot-get)。

```text
/manage → [Doctor]
检查范围：Group「项目协作」

Topic 功能：已启用
Bot 管理员：否
创建 Topic：不可用

建议：将 Bot 设为管理员，并授予“管理话题”权限。
[重新检查] [返回]

# 权限补齐后点击“重新检查”。
Bot 管理员：是
管理话题权限：已授予
创建 Topic：权限条件满足
```

诊断应区分权限条件满足与实际通信成功。Bot 无法连接 Telegram 时的本地诊断入口、是否提供收发探测，待确认。

语言设置按 User 保存，默认跟随 Telegram，也可手动选择；菜单使用打开者的语言。语言字段缺失时沿用上次记录，无法选择支持的语言时回退到 English。名称与对话正文保留原文。

```text
/manage → [个人设置] → [语言]
当前：跟随 Telegram（简体中文）
[跟随 Telegram] [简体中文] [English]

User 点击：[English]
Language: English
[Follow Telegram] [简体中文] [English]
```

管理命令、菜单操作、Bot 的管理回复及结果保存在 DB，不进入 Agent 的 unread/history，不触发 wait。

## 下一轮需要确认

1. 配置文件与 CLI 连接信息如何提供，变更如何生效；是否采用上述初始化状态。
2. Agent 名称允许的字符和长度范围。
3. 未信任消息如何处理，撤销信任如何影响正在进行的操作。
4. Topic 关闭后，新订阅和 wait 如何处理；Telegram 中的外部变更如何反映到网关。
5. 是否采用 5 分钟的 active 判定，以及范围内 waiting 的统计口径。
6. 菜单被其他 User 点击时的行为，以及 Telegram 不可用时的本地诊断入口。

本草案先讨论可观察行为；部署命令、HTTP 传输和 DB 结构留给后续实现文档。
