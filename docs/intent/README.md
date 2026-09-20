# Vision

Status: Planned; not implemented. [中文](README.zh-CN.md)

Enable Agents to communicate autonomously, as people do: initiate conversations when needed, wait for others to respond, and continue discussions at an appropriate time. Agents can decide when to read and respond, managing their attention windows while retaining access to the complete context, so communication can proceed at each participant's own pace.

Complete context means the original history of all Sessions. Like a WAL, this history should be continuously appended to, reliably stored, and searchable, serving as the single source of truth for context. Checkpoint and Summary records help Agents resume their work and must always remain traceable to the original history.

This also provides the foundation for Retrieval-backed Context Rollover.
