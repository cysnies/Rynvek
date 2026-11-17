<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->
# 私有程序化音效制品：真实Arena消费

第三十五批闭合的是**可编辑音色/包络/混音的程序化音效包**，不是只有manifest的静态演示，也不是已完成通用媒体采样链。原24个冻结缺省配方保持原值；包可有界覆写实际振荡器/滤波/包络参数和原manifest gain/cooldown/voice上限，下载数据真正进入原Arena `AudioDirector`/mixer/synth。没有生成音乐/第三方音色，没有解码或播放任意网络音频；`.ogg`采样/分轨音乐/空间化与设备体验仍是后续项，本profile明确拒绝这些输入，不以不安全的头检查冒充恶意媒体解码沙箱。

## 实际入口

原 `/arena` → 连接本人真实席位 →「私有程序化音效制品」。可显式发布SDK生成的已签名JSON、发现本人不可变版本、校验下载/应用、下载原RYPK、撤销、恢复内置配方。无需开新世界、不造对手、不拿控制权；音效仅客户端观察，不发世界命令。练习/观战/回放不提供该私有消费入口。

**应用绝不创AudioContext或播放**；替换包先切断旧图，需再次点击原「手势启用音频」。仅之后新到达的原权威己方阵营事件进入原调度，不补播；原fog过滤/真实脑态/静音/失焦/隐藏/过期/终局/断线/换世界行为保留。身份/世界切换中断下载并丢弃迟到结果，换世界清掉已应用包；私人数据不进localStorage。神经声化原真实指标不改，不把新配方称新的spikes/相干度。Arena只触发当前已接的真实事件子集，不声称24类都已有权威事件来源。

## 权威容器 / 数据与资源边界

复用当前RYPK v1（`rpack`，非另造压缩/可执行格式），`engine_compat=rynvek-audio-recipes/1`；容器≤64KiB，HTTP上传JSON≤128KiB。恰好四个`data/application/json`文件：

- `audio.json`：`{"schema":"rynvek.audio-pack.v1","runtime":"procedural-recipes/1","role":"soundpack"}`；库为`library`。
- `manifest.json`：原`rynvek.audio-manifest.v1`的有界事件覆写，只允许原24事件的`placeholder`及其原总线。gain 0–1，cooldown 0–60000ms，max_voices整数1–16。未覆写事件继续原默认。
- `recipes.json`：事件→配方字段覆写。只允许原24事件，禁止bus或任何未知字段。oscType为sine/square/sawtooth/triangle；filterType为lowpass/highpass/bandpass；频率/截止20–8000Hz，attack1–50ms，decay/release各1–500ms，sustain0–1，Q 0.1–5，noiseMix0–0.8。冻结缺省表不修改，调用者得到独立合并映射；单个声音最长1050ms。客户端和服务端均校验。
- `dependencies.lock`：`{"lockfile_version":1,"packages":[{"id":"owner.sound-library","version":"1.0.0","blake3_256":"…64位小写hex…"}]}`。同owner音频library，最多4直接依赖、8个唯一传递依赖、4层；依赖原容器总量≤256KiB，加根最多320KiB。禁止重复直接ID、缺失/CID冲突/撤销/跨owner/非library/环。按锁声明顺序深度优先后序应用，共享节点只应用一次；后库覆盖先库，根最后覆盖，配方按字段合并、manifest按事件条目覆盖。

容器继续权威路径/偏移/长度/排序/保留位/整包与每条目BLAKE3检查；四文件profile不接受额外路径、源码、媒体blob、外部URL、动态模块、权限/凭据字段。保守凭据标记拒绝复用原Agent scanner，不冒称可检测任意编码秘密。新schema见`protocols/schemas/audio-pack.schema.json`（profile根、recipes/manifestPatch定义）；数值和总线权限同时由实际Rust/TS校验器执行。

只有包数据改变实际音色，不得改变能听到的权威信息：原`planArenaAudio`只接真实授权阵营投影并再次过滤敌方事件，制品无权请求观察、注册额外事件源、访问世界或网络。复用原总线/事件发声预算，音色参数不拓宽信息或工具权限。

## 私有签名 / Store / API

Ed25519消息为UTF8 `rynvek-private-audio/1\0{owner_u64_decimal}\0{id}\0{version}\0` + 原制品32字节BLAKE3，与Agent/公共Hub签名域分离。签名覆盖整个容器，不收集或注入私钥。SDK签名回调仅收待签名字节；公钥锚定于已认证服务元数据，不是独立CA。上传若不确定成功，先发现版本对账，不能盲重发或新造版本。

- `GET /api/v1/private-hub/audio`：本人版本（含撤销墓碑），最多256条。
- `GET|PUT|DELETE /api/v1/private-hub/audio/{id}/{version}`：元数据/显式不可变发布/撤销。PUT体为`{artifact_b64,signature}`。
- `GET .../artifact`：签名原制品；私有、no-store、nosniff。SDK有界流读取并再次验BLAKE3/Ed25519。
- `GET .../resolved`：仅soundpack根；重新验证根/所有精确依赖的私有CAS、签名、profile和撤销，返回真实合并manifest/recipes。SDK先校验下载根和所有锁定制品，再消费认证服务的解析结果，不声称独立客户端重做全部解析。

发现/读取需原`WorldRead`，发布/撤销需原`HubPublish`，均按令牌主体owner隔离，未知与他人404。公共Hub列表和公共blob路由不可见；不公开发行/外部部署。元数据与CAS保存路径沿用原私有包模式，`PrivateArtifactRow`复用原记录形状，Agent原序列化不变，仅Rust内部名字准确泛化。

Store仍原Memory锁/PG事务，独立`0008_private_audio.sql`表，不复制执行作业框架：同主体行锁串行配额/发布，事务重验active发布者/公钥、所有精确依赖未撤销且CID一致；同owner/id/version不可覆盖，包括撤销墓碑，每owner最多256版本。依赖校验后撤销的竞态由同锁/PG事务防护；私有CAS预写孤立对象可残留，不自动删除。data-export包含本人音频元数据。

撤销阻止后续解析/下载/新依赖发布，当前面板撤销会切断音图；**已下载字节无法收回**，没有跨标签即时DRM或远程抹除承诺。已在别处加载的数据仍是本地数据；换世界/身份会清本会话。Memory元数据不跨服务端OS进程持久化，真正跨进程需原PG、持久私有CAS、稳定鉴权；PG迁移/SQL本批只Rust编译，未接实库，更不是多机灾备。

## 最必要SDK消费

沿原`PrivateAgentHubClient`已认证同源transport复用授权/no-store/credentials omit/redirect error，不另收令牌：

```ts
import {signPrivateAudioPackage} from "@rynvek/agent-sdk";
const audioHub = hub.audio();
const upload = await signPrivateAudioPackage(ownerDecimalString, {
  id:"owner.quiet-bank", version:"1.0.0", name:"本人有界音效",
  license:"CC0-1.0", role:"soundpack",
  manifest:{schema:"rynvek.audio-manifest.v1",events:{
    "building.placed":{status:"placeholder",bus:"sfx_world",gain:0.17,max_voices:1}
  }},
  recipes:{"building.placed":{freqStart:733,freqEnd:222,releaseMs:77}}
}, explicitSignerCallback); // 不传私钥给SDK；须明确自有内容/私有发布授权。
const version = await audioHub.publish(upload);
const found = await audioHub.list();
const bytes = await audioHub.download(version); // 原签名容器，自动验完整性。
const bank = await audioHub.load(version);       // 精确依赖校验 + 私有服务解析。
arenaAudio.setSoundBank(bank.manifest,bank.recipes,`${version.package_id}@${version.version}`);
// 仍静音；只能在真实按钮的同步手势栈另调arenaAudio.enableFromGesture()。
```

SDK所在包仍为已有agent-sdk的私有制品控制面扩展，不代表音频数据会交给Agent/provider；`AudioContext`、播放手势和fog仍只在原Arena消费层。没有新公开npm/NuGet发行或第三方音色依赖。

## 实际直接验证 / 尚未验证

11个不同直接用例（重跑不累计）：Rust恶意数据/容器拒绝1；真正本机认证HTTP→Node SDK构建/签名/私有发布/依赖→原Arena WS/人类租约/真实建造命令→权威building_placed→实际ArenaAudioSession/Director/mixer/synth读取下载配方1；音频数据/原型字段/真实参数2；原Arena音频生命周期5；新面板显式应用/身份迟到隔离2。真实服务集成不是假事件：声卡是录制型FakeAudioContext，实际733Hz/222Hz振荡器参数与0.17 trim来自下载制品；真实Actor命令有building_placed ack。验证重复版本拒绝、错CID/签名、他人/匿名隔离、公共blob不可见、撤销依赖阻止解析及新发布、data-export。测试身份仅stdin传递，不写入文件/URL/日志或制品。

```sh
pnpm --dir packages/audio exec tsc -p tsconfig.build.json
pnpm --dir packages/agent-sdk exec tsc -p tsconfig.build.json
pnpm --dir apps/web-platform exec tsc --noEmit
cargo check -p rynvek-server
cargo test -p rynvek-server --test private_audio -- --nocapture
pnpm --dir packages/audio exec vitest run test/sound_bank.test.ts
pnpm --dir apps/web-platform exec vitest run test/arena_audio.test.ts test/private_audio_panel.test.ts --config vite.config.mjs
git diff --check
```

这是必要编译和直接小范围快验，不是全workspace/Clippy/license/SBOM/soak/CI轮询。Node仅加载受信仓库自己的实际ArenaAudioSession（TS转译后导入），从不执行包源码。服务端为本机MemoryStore，Tick由夹具有界主动推进，非真实时序/服务端重启/PG灾备测试。没有实际浏览器策略/声卡/耳听、DSP离线波形渲染、PG实库多机、断电/长稳验证，不把录制节点声称听到了声音。

下一转一般规则/参数模板的真实Studio→权威应用→Lab消费段，再按依赖补一般W3/协作和未覆盖完整生命成本；正式采样/空间化等音频欠项保留，全项目不宣称完成。
