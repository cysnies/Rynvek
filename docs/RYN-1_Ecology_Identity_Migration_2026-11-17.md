<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# RYN-1：生态模式身份绑定与显式历史迁移

## 依据与问题

项目v1的D044/D063要求候选事务失败原子回滚；D048及D322–D327要求保留来源的Copy-on-Write分支和WAL恢复；D189–D195及开发规范的CommitManifest要求真实前驱根。Schema迁移不能伪造历史状态哈希。本批修身份正确性，不增加生态规则、人工适应度或另一套模拟器。

实际红测确认：旧纯energy_exchange_v1和forager_budget_v2只追加相同energy_exchange_v1标签，新建世界config_root相同。旧根中缺失的信息无法从根本身重新推导；不能将快照自述模式当作独立可信证据，也不能靠重封装容器校验和弥补。

## 新世界与格式

- 不新增EcologyVersion。新增MatchConfig中的根编码标记ecology_identity=bound_v1；它是根格式，不是第三种生态实验模式，也不是第二份生命状态。
- ArenaWorld新建入口始终选择当前编码；真实POST /api/v1/worlds把实际最终配置写入原创世CAS记录。新生态config_root独立域绑定实际生态枚举和完整可选生存参数。
- 宿主MatchSpec须使用真实创世配置或明确绑定的新建配置；仍保留原完整相等比较，不能把旧格式模板当作当前世界恢复配置。原能量服务三例曾因模板缺新身份而失败，修调用端的实际配置，不放松宿主校验。
- 无生态的既有规范根不变；旧显式生存契约已有完整绑定的根算法也保留。默认恢复不会将历史编码升级为当前编码。
- ArenaRuntime普通事务拒绝更改整个MatchConfig；不能通过WorldPatch/候选事务暗换生态模式或根编码。旧歧义生态也不能用普通fork绕过显式迁移。

## 旧根读取/回放的信任前提

1. SnapshotReader继续只做通用容器读取；不擅自决定模式。
2. 对缺根编码、无生存契约的旧纯V1/V2，ArenaWorld::decode_snapshot和ArenaRuntime::restore默认拒绝。显式decode_snapshot_with_config/restore_with_config要求**独立可信**原配置完整一致，保留旧根和原规则执行。
3. 服务恢复从数据库已提交创世journal CID读取配置并校验CID，随后锁定该配置恢复检查点和TickRecord后缀；不是从待验CONF反推期望。原有提交链/CID/修订/根检查继续保留。
4. 旧歧义TickRecord的无上下文replay拒绝；只有replay_with_config的独立可信上下文可回放。新绑定模式用原base_root/state_root即可拒绝跨模式/跨迁移混用，不重复保存一份模式状态到每条WAL。
5. **限制不可掩盖**：旧根/旧原始WAL本身不包含模式证据。若独立创世契约缺失（例如某些旧fork把任意Tick日志当创世记录）、来源被整体替换或调用者用快照自述伪装可信期望，不能证明历史模式。本批服务对歧义且无可信创世配置的数据拒绝，不猜测、不自动救活；低层读取仍保留，原始数据不删除。已绑定旧生存契约和无生态旧格式无需此歧义例外。

## 显式迁移事务

真实入口：POST /api/v1/worlds/{world_id}/migrate-ecology-identity。

请求必须完整提供base_revision、base_root（32字节数组）、expected_config。服务要求来源WorldEdit和目标个人组织WorldCreate权限，沿既有私有世界防探测策略；拒绝活动宿主租约和过期来源基线。来源仍必须通过创世CID锁定恢复，不能靠请求自称模式获准。

成功只创建一个**私有新世界分支**：

- 来源生态模式/生存参数、仿真Tick和全部物理/神经状态不变；只变根编码和新世界身份。
- 新分支修订0，CommitManifest.parent_commit_hash明确等于来源真实旧根；不重写来源根、检查点、WAL或历史修订。
- 原insert_world数据库事务同时发布新世界与创世提交。持久化/配额拒绝不发布半迁移世界、不动来源，最多留下不可达CAS对象，不删除它们。
- 创世journal CAS保存rynvek-ecology-identity-migration/1清单，包含目标实际config、来源世界/修订/Tick/真实根/journal CID和原配置。它随提交持久化，不只依赖尽力型audit日志；成功另记普通audit事件。
- 新分支可继续原TickCommitter/检查点/WAL恢复，旧来源日志不能直接混入新根链。该接口不是规则升级、原地历史重签名或宿主抢占接口。

## 验证范围

- 保留原实际根碰撞红测断言，修新建根算法。
- 原V1/V2各自冻结快照及12个后续根不改；改用独立固定已知原配置显式恢复，并断言无上下文恢复拒绝。无生态旧黄金夹具不变。
- 合法重封装跨模式/降级快照仍拒绝；错误历史模式pin拒绝；迁移前后源检查点字节不变、真实前驱根不伪造、迁移后12步及再次恢复确定一致。
- 真实HTTP迁移权限、防探测、错误模式、活动租约、私有分支、可持久审计清单和迁移后WAL恢复；用既有组织真实配额触发insert_world事务失败，确认来源行/根/提交/字节及目标世界数量不变。
- 真实旧V1/V2服务检查点加WAL锁定恢复；用真实旧格式根碰撞构造模式被改但创世配置未改的记录，服务必须拒绝；新模式空输入WAL也拒绝跨模式。

最终HEAD完整门禁和CI快照记录于同一Page。本批不宣称浏览器、硬件GPU、真PG、断电或24小时验收，也不宣称自然生态整体完成。额外强制Python lib-test链接缺开发共享库限制与规范Python扩展行为门禁分别报告。

## 本批CI一次快照

2026-11-17 17:48:03中国标准时间起的一次查询：

- 44048056487已success，job于17:13:28结束，实际37分53秒；Full Gate 36分24秒。
- 44052057093已success，job于17:38:33结束，实际34分59秒；Full Gate 33分22秒。
- 44058326709当时仍in_progress、无最终结论；job约3分35秒，Full Gate自17:45:35起约2分29秒。准备依赖13秒、真实WASM/ABI40秒已success。

没有已结束失败，故无失败日志可修；不把长耗时猜成失败，不轮询或取消其他任务。新最终HEAD推送后仅单次读取新run。
