<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 私有PCM采样与原Arena二维空间化（第四十批）

从9bf6584继续，闭合**签名真实采样容器→有界扫描→精确私有依赖→发现/下载/验证→原Arena手势图→真实可见位置/渲染相机声像和距离衰减**。不是未消费的文件格式，也不是工业美术/音乐或硬件听验。视觉registry/.rview与主题独立，下一主体推进，不在音频阶段塞不相干空壳。

## 实际可用入口

`/arena`连接自己的实际控制席位 →「私有采样/程序化音效制品」：发现本人精确版本、校验下载并应用、查看实际清单/许可证/署名、下载签名原制品；已有显式私有发布/撤销继续使用。应用只替换内存数据并关闭旧图，**仍须另点上方启用音频**，无自动播放/历史补播。UI上传已签名JSON上限768KiB，无私钥。标准按钮手势同步创建/请求resume，之后才预载本地已验证PCM；失败不假装采样就绪或自动绕过浏览器策略。

发布仍是原`PUT /api/v1/private-hub/audio/{id}/{version}`；本人发现、元数据、artifact、resolved与撤销路由不变，WorldRead/HubPublish及owner隔离不扩权，公开Hub不列出。应用端/SDK复验根与精确依赖的BLAKE3、发布者Ed25519及撤销，原Store/CAS/PG实现复用，无新表/部署。服务端是解析信任锚，不声称独立CA；许可证白名单/署名校验不替代真实版权授权审查。

### SDK生成自有CC0技术样本上传对象

以下只构造返回对象，不自动发布/外发；`sign`由调用者显式提供现有Ed25519 signer，不读/保存/打印密钥：

```ts
import {createTechnicalPcm16Wav} from "@rynvek/audio";
import {pcmSamplePath,signPrivateAudioPackage} from "@rynvek/agent-sdk";
export async function technicalUpload(ownerId:string,
  sign:(message:Uint8Array)=>Promise<Uint8Array>) {
  const wav=createTechnicalPcm16Wav(); // 16kHz / 80ms / 733Hz / 20% envelope
  const src=pcmSamplePath(wav);        // samples/<真实BLAKE3>.wav
  return signPrivateAudioPackage(ownerId,{
    id:"owner.technical-audio",version:"1.0.0",name:"CC0技术信号",
    license:"CC0-1.0",role:"soundpack",recipes:{},samples:{[src]:wav},
    manifest:{schema:"rynvek.audio-manifest.v1",events:{
      "building.placed":{status:"asset",bus:"sfx_world",src,
        license:"CC0-1.0",credit:"Rynvek self-generated technical signal; CC0-1.0",
        gain:.17,max_voices:1,cooldown_ms:500}
    }}
  },sign);
}
```

此函数生成的是短校准测试信号，输出明确CC0-1.0；不是歌曲/外采音色，无第三方媒体或网络资产。示例JSON可在自己的程序保存后由现有UI显式发布到本人私有库；不公开发行。也可使用原`PrivateAgentHubClient.audio().publish/load`，导入函数不是第七工具。

## 当前v2受限格式与资源

- 仍为原RYPK v1容器、原`rynvek-private-audio/1`签名信封。新音频profile：`rynvek.audio-pack.v2` / `pcm16-recipes/2` / `rynvek-audio-pcm16/2`。
- 四个必需JSON：audio.json、manifest.json、recipes.json、dependencies.lock；另只允许最多8个内容地址`samples/<64小写hex>.wav`，data/application/octet-stream。每个包上限512KiB，无源码、任意路径、压缩、网络src或动态脚本。
- **仅规范44字节头的RIFF PCM16 little-endian、单声道、无额外块**。采样率白名单8000/16000/22050/24000/44100/48000；每clip最多2秒、192044字节；根及解析后唯一采样合计最多192000帧/8项。字段、RIFF/data尺寸、byteRate/blockAlign、格式码、位深、帧数、内容地址逐一核验；不是通用WAV/Ogg/MP3/浮点PCM解码器。
- 原精确library依赖最多4直接、8唯一节点、4层、依赖字节4MiB，根另限512KiB；合并后仍受8采样/192000帧总上限。样本用内容地址消除名称覆写，JSON规则按原后序/root优先合并；缺样本、冲突、缺精确依赖、撤销均拒绝，不到网络兜底。
- resolved内置Base64样本；SDK先限数量/编码量再解码并核对样本内容地址，音频消费者再次严格PCM头/帧/总量校验，Float32只按i16/32768生成。AudioBuffer仅在手势图创建后从已验证帧填入，不调用decodeAudioData、fetch音源或平台媒体编解码器。
- 事件仍限原24类及固定原总线，禁止包改变语义/总线、相机、坐标、控制仿真或拓权。原事件冷却、每事件≤16、原五总线8/16/24/2/12活跃声部上限；单采样≤2秒且不loop。采样结束显式断开source/gain/panner。节点/内存有界不是长稳或声学安全认证，系统音量由用户控制。
- 素材仅CC0-1.0或CC-BY-4.0；后者必须非空≤256 UTF8字节credit，UI可查看，文件原文随包。原配方仍可与采样混用，不执行任意脚本。
- 不保留旧私有音频profile运行适配；旧v1数据/签名/CAS/Git历史保留、不自动迁移，当前发现只列v2，旧profile加载显式拒绝。一般AudioManifest可描述ogg/wav，但本私有Arena链**仅上述严格PCM**，不因此启用Ogg/网络播放器。仿真Arena/11、CURRENT_11、WAL/2与Lab kernel/4不变。

## 空间化与fog边界

原AudioSession从实际当前阵营观察读取源：本人且仍在当前units/buildings中的ID，以及原fog聚合格**1=当前可见，2=仅探索，0=未探索**。单位从原Q32坐标转换为呈现float；建筑使用实际origin锚点。不读ghost，不从消失/前帧/敌方事件推断死亡位置，不向隐藏格发声。

生产`browser.ts`直接读取原`arena.ui.camera`和同一个`view()/viewScale()`，包括GPU有效缩放/DPR夹限，半视口以实际canvas尺寸计算。每次新cue在触发时计算`pan=clamp((x-camera.x)/halfWidth)`，距离衰减`1/(1+dx²+dy²)`；采样与原合成器同样应用。无有效源/相机/视口则不造假居中世界音源。少数UI/科研/已观测主脑HP下降告警是明确非定位通知；不宣称知道攻击者、发射或爆炸位置。

这是**二维立体声声像+距离衰减**，不是HRTF、3D遮挡/反射/Doppler或持续追踪移动声源。当前死亡事件不带可信当前位置，已消失源不补历史位置音效；观战/回放音频仍未接。神经声化仍仅真实己方spike/连续读出，不造spikes或Kuramoto指标。

静音/隐藏失焦/挂起/3秒无新Tick/错误/终局同步切断图；断连/换世界/身份清采样库与私有结果。换包也关闭旧图且需新手势；resume/preload迟到有epoch防复活，等待期间不积压事件。撤销阻止新服务端解析/下载，UI本次撤销切断当前图；已下载字节不能远程追回，不称DRM或持续在线撤销推送。

## 必要直接验证与诚实边界

13个不同直接用例（新增5，重复不累计）：PCM/真实Director节点3，协议一致性1，Arena原生命周期+可见位置/相机/fog隔离6，Rust扫描2及真实本机认证HTTP→Node SDK签名根/library→私有发现/校验下载/解析→原Arena WS真实建造→实际PCM声源图1。后者验证非零1280帧/16000Hz、真实新建筑origin与受控测试相机的pan=.25、原混音增益/距离衰减、结束回收、owner隔离/本人数据导出元数据与依赖撤销；生产相机来自真实renderer共用view，Node快验不是操作浏览器。

Audio/SDK必要tsc build、web类型、server必要编译及diff检查。**录制AudioContext只是节点/参数与生命周期验证，不是真实浏览器Web Audio DSP、OfflineAudioContext声学渲染、耳机听验、实际GPU或长期稳定证据**。纯CPU PCM数学解码与原本机服务调用是真实执行。无全workspace/Clippy/license/SBOM/soak/CI轮询；未跑PG实库/多机/断电/长稳，不以MemoryStore/文件CAS当灾备。无外部发行部署、额外依赖、外采素材、第三方模型调用或网络浏览器策略绕过。

下一主体：视觉registry/签名受限.rview主题与原renderer实际消费；生命成本剩余通用生长/材料体量、EMIT、连续神经/规则计价，以及一般TS/rpack/物理ECS、独立Agent赛道/真实LLM、协作presence/离线多主等继续保留。Pi仍既有MIT设计借鉴、未引代码；全产品未完成，主体全齐后集中全面验收。
