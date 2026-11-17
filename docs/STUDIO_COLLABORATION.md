<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->
# 定向私有Studio协作与原日志同步

2026-11-18，中国标准时间 UTC+8；第三十七批，从dc7af6c继续。

## 真实操作入口

`/studio` → 所有者以正式WorldEdit凭据连接Private世界 →「私有Studio协作 / 原日志同步」：

1. 刷新真实创作准入，填写已存在的正式主体ID，明确勾选「读取源码并编辑此私有世界」，显式授予。没有发送站外邀请/通知，没有假在线成员或自动公开世界。
2. 获授权者使用**自己的**WorldEdit凭据和该世界ID连接原Studio，选择「每秒消费已提交日志」或手动同步。所有者也可消费同一日志。准入清单不是在线状态。
3. 编辑原地形/参数/基因/模块，或上一批一般参数模板/实例/绑定；仍经原Epoch/Actor或非活动完整检查点事务。真实运行须由所有者原Arena入口启动，编辑者准入**不授予控制席位/租约/自动开局**。
4. 同会话同步保留本人选择性撤销历史；协作者后来覆盖的寄存器跳过，不能撤销别人的胜出写。跨对象引用的逆操作仍经原权威整包校验，合法选择性逆补丁不保证一定满足当前模型依赖。
5. 断线/撤权/提交冲突使会话失效并清私有画面/草稿/身份；显式重新连接读取当前基线，撤销栈重置。无离线写队列、盲重试或断网自动覆盖。

同步不是新的仿真：原Store CommitRow定位原私有CAS，提取原WorldPatch/TickRecord Patch输入；原已提交revision/tick/root为事实源。SDK保持独立同步游标，不把补丁数冒充世界revision，原Tick无编辑也推进游标。每页最多64条提交，按原日志字节优先限制分页（单条受原输入队列上限），SDK一轮最多8页，剩余下一轮继续；原Store list_commits仍全元数据扫描，非大历史索引性能认证。

## 权限与撤销

`GET/POST /api/v1/worlds/{world_id}/authoring/editors`仅Private世界所有者：

```json
{
  "principal_id": "正式主体精确十进制ID",
  "expected_grant_id": null,
  "active": true,
  "consent": "studio-source-edit/1"
}
```

初次expected=null；更新/重授/撤销须原grant_id。每次操作换版本，旧版409；16个有效编辑者、64个累计主体记录（不自动删墓碑）。本权限**明确包含该世界全部Studio可读创作源码（共享基因、规则、地形、参数）及当前已支持编辑**，不适合作为只读fog观战替代。

只开放该世界authoring/compile/patches/changes，不给一般WorldRead/Edit资源权限、其它私有世界、完整commits/快照/归档、fork、Arena控制、Hub或准入管理权。编辑者还须自己的Active身份及WorldEdit scope，所有者也必须Active。源码已合法下载后不能从人的记忆撤回。

创作基线返回`collaboration {principal_id:string,role,grant_id:null|string}`；SDK随后通过`X-Rynvek-Studio-Grant`绑定同一版本。旧会话不会在撤销后静默接受新grant；重授须新建正式连接。UUID只是准入版本，不是凭据；Bearer仅内存和认证头，禁止redirect/Cookie/存储。

HTTP开始、Actor入队/执行、**原Store提交同一锁/PG事务**均重验不可变准入版本。撤销/重授同世界锁串行化，撤销先完成则旧候选不能提交。排队待Epoch的已撤权补丁拒绝、不会终止所有者整局；若撤销恰与提交阶段竞争并使已计算候选失效，则整Tick不发布，保守仅退役本Actor（不发持久Aborted/Completed/评分），所有者须原入口重连已提交世界，避免复用未发布的工具/门禁状态。已先提交的编辑不能被撤销操作追溯抹除。提交后再被撤权的HTTP回执可能失效，按结果不确定处理，不重发。

## CRDT与权威冲突边界

原HLC/LWW/UndoManager保留；在线投影按**真实提交revision**定序，而不相信客户端未来墙钟。只在客户端物化历史中归一排序戳，原WAL载荷/patch ID不重写。`author_node_id`不是可信身份，真实请求主体走服务认证及原审计；审计是附加记录，不冒充原世界提交的原子作者签名。

这是单条权威链上的协作，**不是多主离线CRDT任意合并**。同一过期创作基线的并发提交仍原409，只能刷新/同步确认后显式新提交；本批不自动重放失败写。复杂撤销可能因新的模型依赖被422拒绝，不能为了撤销越过权限或权威原子性。

SDK：`StudioClient.editors/changeEditor/changes`、`RemoteStudioSession.sync`。同会话最多512条或8MiB本地补丁历史，达限显式重连清撤销边界，不无限堆内存。连接/同步失败后UI清私有状态；自动同步是显式启用的每秒只读HTTP分页，不谎称WebSocket推送、光标/在线状态或实时低延迟SLA。

## 存储与验证边界

独立幂等`0009_studio_editors.sql`，原MemoryStore锁与PG同世界行锁/事务。未改原规范DDL，不另造Actor、日志或模型。Arena/10、CURRENT_10、WAL/2、Lab kernel/2均不变；第三十六批旧/9拒绝/不自动迁移边界保留。

实际定向命令：

```sh
cargo check -p rynvek-server --offline
pnpm --dir packages/studio exec tsc -p tsconfig.build.json
cargo test -p rynvek-server --test studio_collaboration --offline -- --nocapture
pnpm --dir packages/studio exec vitest run test/collaboration.test.ts test/remote.test.ts
pnpm --dir apps/web-platform exec vitest run test/studio_workspace.test.tsx --config vite.config.mjs
pnpm --dir apps/web-platform exec tsc --noEmit
```

本批16个不同直接用例通过（Rust2、SDK2+原remote5、原Studio JSDOM7），重跑不累计；必要server编译、Studio SDK tsc build、web tsc --noEmit与diff检查通过，最终真实服务用例于2026-11-18 03:26:09中国标准时间结束。

真实HTTP注册的所有者/编辑者、两套独立认证SDK会话、原WS/Actor验证一般模型编辑/同步、撤销跳过他人后写、undo/redo、过期409、撤权/重授版本隔离、其它世界和元数据/commits拒绝、原WAL逐条回放同根；另有真实排队后撤权用例。测试不联系任何外部协作者。

节点上下文重建测试保留同MemoryStore、真实私有磁盘CAS和稳定测试签名器，重新启动本机HTTP服务、读取原准入/日志/模型；不是Memory跨OS进程持久性、硬杀/PG多机灾备。PG SQL只Rust编译，未连实库；生产跨进程需原PG/持久共享私有CAS/稳定鉴权。真实浏览器、PG迁移/并发SQL、故障分区/断电/长稳、大历史容量未验；JSDOM只验表单和生命周期。无全workspace/Clippy/license/SBOM/soak/CI轮询或网络策略绕过。

一般参数模型Lab/W3导入与四表统计**仍未接通**，本阶段优先完成独立高价值协作治理，不冒称已完成实验消费；这是下一主体阶段。完整生命成本、一般TS/rpack/.rview、音频采样/空间化和视觉制品等仍欠，全项目未完成。
