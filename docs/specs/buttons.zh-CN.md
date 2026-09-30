# Agent 按钮与消息编辑

状态：接口契约，当前实现范围见[项目 README](../../README.zh-CN.md)。[English](buttons.md)

Agent 可以为消息添加按钮，轮询按钮点击事件，再编辑同一条消息展示下一页。Gateway 负责传输、消息归属校验和回调持久化；应用负责页码状态、用户及会话授权、业务操作。本文定义这些基础接口，不提供应用轮询进程或数据源集成。

## 为消息添加内联键盘

`send` 接受可选的 `--reply-markup '<JSON>'`。参数是 JSON 对象，`inline_keyboard` 按行组织按钮：

```json
{
  "inline_keyboard": [
    [
      {"text": "Next", "callback_data": "pg_7f2a"},
      {"text": "Help", "url": "https://example.com/help"}
    ]
  ]
}
```

每个按钮必须有非空白的 `text`，并且只指定一种受支持的动作：`callback_data` 或 `url`。`callback_data` 按 UTF-8 编码后须为 1–64 字节，不能按字符数判断。URL 按钮接受 `http://`、`https://` 或 `tg://` 地址。其他 Telegram 按钮类型与字段不在本接口范围内。键盘格式不合法时，send 在接收前报错。`{"inline_keyboard":[]}` 表示没有按钮；编辑时传入它会清除键盘。结构沿用 Telegram 的 [InlineKeyboardMarkup](https://core.telegram.org/bots/api#inlinekeyboardmarkup) 和 [InlineKeyboardButton](https://core.telegram.org/bots/api#inlinekeyboardbutton)，Gateway 仅开放上述子集。

send 仍在本地持久接收后返回 `{"msg_id":"m42"}`，既有队列、重试与投递语义不变。编辑要求消息已完成投递，并已有 Telegram 消息映射；send 返回成功还不满足编辑条件。

## 独立读取按钮回调

```text
tbg --agent <name> callback list [--topic <topic>] [--cursor <callback_query_id>] [--limit <n>]
```

该命令要求已注册的 Agent，只返回属于该 Agent 所发消息的回调。省略 `--topic` 时跨该 Agent 的 Topic 读取；不要求或自动建立对话订阅。回调按持久化接收顺序从旧到新返回，默认最多 20 条：

```json
{
  "callbacks": [
    {
      "callback_query_id": "cq-73",
      "topic": "<topic>",
      "msg_id": "m42",
      "user_id": 12345678,
      "data": "pg_7f2a",
      "received_at": "2026-09-30T09:10:00Z"
    }
  ],
  "next_cursor": "cq-73",
  "remaining_count": 0
}
```

`callback_query_id` 是 Telegram 的不透明查询标识，也用于回答该次点击。`msg_id` 指向 Gateway 中原有的出站消息，`user_id` 是点击者的 Telegram ID。`received_at` 记录 Gateway 接收时间，排序依据持久化的接收序列。

游标排除它所指的回调，只读取后续记录。`next_cursor` 是本批最后一条查询 ID；结果为空时保留传入的游标，未传游标则返回 null。`remaining_count` 统计查询时尚未返回的后续匹配回调。每次读取重新查询当前状态，分页间没有固定快照；继续读取时应保持同一 Agent 和 Topic 筛选条件。读取和回答都不保存消费进度，应用应在处理完本批回调后再保存游标。保存前中断可能导致重放，业务副作用须按查询 ID 去重。重复收到同一 Telegram update 或查询 ID 不会追加另一条回调。

回调不进入普通对话，不出现在 `unread` 或 `history`，不触发 `wait`，不推进 ack，也不产生 ❤️ 回执。即使应用已经等待对话，仍须独立轮询 `callback list`。

### 接收条件与业务授权

Gateway 仅接收已接入 Group 中管理员或可信 User 的回调，且必须能映射到已知的 Agent 出站消息，`data` 必须匹配该消息当前保存的键盘按钮。Telegram 只提供不可访问消息引用时，仍可通过 chat ID 和 message ID 完成映射。inline mode 和游戏回调不受支持，URL 按钮点击也不进入该回调流。

通过上述校验不代表用户已获业务授权。回调数据应按不可信的不透明输入处理；应用使用前须核实点击者、Topic、当前页面或会话以及操作权限。已保存的回调也可能在编辑后过期。按钮中应放指向应用状态的短 token，不应嵌入私人记录或个人账本数据。Telegram 也提示回调数据未必与当前消息匹配，见 [CallbackQuery](https://core.telegram.org/bots/api#callbackquery)。

HTTP 仍不鉴权。Agent 归属校验防止跨 Agent 误路由；能够访问 Gateway 的调用方仍可选择已注册的 Agent 名称。接口应仅在本机或受控私有网络中开放。

## 及时回答点击

```text
tbg --agent <name> callback answer <callback_query_id> [--text "<text>"] [--show-alert]
```

只有消息所属的 Agent 能回答已保存的回调。可选文字最多 200 个字符；`--show-alert` 用弹窗代替通知。成功返回 `{"callback_query_id":"cq-73"}`。Gateway 同步调用 Telegram 的 [answerCallbackQuery](https://core.telegram.org/bots/api#answercallbackquery)。即使不显示文字，也应尽快调用，以结束 Telegram 客户端的加载提示。回答不消费回调、不编辑消息，也不确认普通对话。Telegram 可能拒绝已过期的查询。

## 更新原消息

```text
tbg --agent <name> edit text <topic> <msg_id> "<content>" \
  [--format plain|markdown] [--no-header] [--reply-markup '<JSON>']
tbg --agent <name> edit markup <topic> <msg_id> --reply-markup '<JSON>'
```

编辑和回调回答要求 Gateway 在本次启动后已核实配置的 Bot 身份；就绪检查尚未完成时会拒绝操作。Telegram 不可用时，`callback list` 仍可读取已保存的回调。

两个命令都返回 `{"msg_id":"m42"}`，仅可操作活跃且可用 Topic 中自己已投递的消息。正文编辑沿用 `send` 的渲染和长度规则，默认纯文本并带 Agent 标头。需要 Markdown 或省略标头时，须再次传入 `--format markdown` 或 `--no-header`，不会从原消息推断渲染选项。正文编辑省略 markup 时保留已保存的键盘；传入时替换键盘，空 `inline_keyboard` 清除键盘；非空键盘中的每一行都须至少有一个按钮。仅编辑 markup 不改变正文。

Gateway 调用 Telegram 的 [editMessageText](https://core.telegram.org/bots/api#editmessagetext) 或 [editMessageReplyMarkup](https://core.telegram.org/bots/api#editmessagereplymarkup)，在 Telegram 成功后更新保存的原文和/或键盘。原 `msg_id` 和对话位置不变，不新增消息或 ack。已经确认过该消息的 Agent 不会收到新的待处理消息。尚未确认的消息继续按通常的待处理和 @ 提及规则处理，内容以编辑后的版本为准。

编辑和回调回答均同步执行，没有持久化重试队列；Telegram 错误直接返回调用方。Telegram 的“message is not modified”响应按编辑成功处理，使完全相同的重试能补齐本地状态。响应丢失或进程崩溃时，可能出现 Telegram 已更新、本地历史尚未保存的情况。仅当目标页面仍是当前版本时，才重试相同编辑；过期重试会覆盖较新的页面。Gateway 串行执行编辑，保持 Telegram 更新与本地历史的顺序一致。应用仍须串行管理页面逻辑、拒绝过期会话，并处理结果不确定的情况。此前 send 若在 Telegram 产生了重复消息，编辑只作用于保存映射指向的那一条。

## 示例：翻到下一页

页面正文对所有能读取 Topic 的参与者共享。以下不透明 token 指向应用保存的页面和会话状态，不含个人记录。

```sh
# 保存返回的 msg_id；完成投递后才能编辑。
tbg --agent hopeful_morse send <topic> "Page 1 of 2: A, B" \
  --reply-markup '{"inline_keyboard":[[{"text":"Next","callback_data":"pg_7f2a"}]]}'

# 独立于对话 wait 轮询；本例收到 m42 的 cq-73 回调。
tbg --agent hopeful_morse callback list --topic <topic> --limit 20
# 应用核实 cq-73 的 user_id、Topic、token 与会话。
tbg --agent hopeful_morse callback answer cq-73

tbg --agent hopeful_morse edit text <topic> m42 "Page 2 of 2: C, D" \
  --reply-markup '{"inline_keyboard":[[{"text":"Previous","callback_data":"pg_91bc"}]]}'
# 本批处理完成后，由应用保存 next_cursor。
tbg --agent hopeful_morse callback list --topic <topic> --cursor cq-73

# 应用会话结束时移除控件。
tbg --agent hopeful_morse edit markup <topic> m42 \
  --reply-markup '{"inline_keyboard":[]}'
```

示例仅说明 CLI 契约，不代表真实 Telegram 验证结果。应用部署、身份认证和数据源访问由调用方负责。
