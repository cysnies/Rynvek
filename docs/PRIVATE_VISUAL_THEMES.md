<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# 私有受限.rview主题与原renderer消费（第四十一批）

从2082bd3继续，闭合**签名原RYVW容器→有界扫描/精确library依赖→SDK发现下载验签→显式应用→原FrameScene/WorldRenderer真实颜色参数**。不是未消费的格式空壳，也不是最终工业美术或浏览器/独立显卡验收。

## 实际可用入口

`/arena`连接自己的实际控制席位 →「私有受限视觉主题 / RVIEW」：发现本人精确版本、校验下载并应用、查看清单/许可证/署名、下载签名原制品；显式发布/撤销沿用。应用只替换表现层八个不透明颜色（世界背景+七阵营），**须点应用**；不自动拉取、不改fog可见性/几何/仿真/应用UI。UI上传已签名JSON上限96KiB，无私钥。

发布：`PUT /api/v1/private-hub/visual/{id}/{version}`。本人发现、元数据、artifact、resolved与撤销路由对称于私有音频，WorldRead/HubPublish及owner隔离不扩权，公开Hub不列出。SDK/服务端复验BLAKE3、发布者Ed25519及撤销；原Store/CAS/PG事务复用，新表`private_visual_versions`（0010），无新外部部署。服务端是解析信任锚，不声称独立CA；许可证白名单/署名不替代真实版权审查。

用现有`writeRview`生成原RYVW，再`signPrivateVisualPackage`构造上传对象；`sign`由调用者显式提供Ed25519，不读/保存/打印密钥。不自动发布或外发。

## 当前私有profile与资源

- 仍为原RYVW v1容器。新私有profile：`rynvek.private-visual.v1` / `opaque-palette/1` / 清单`engine_compat=rynvek-private-theme/1`；签名域`rynvek-private-visual/1`（制品摘要与规范profile一并签名）。
- 只许1–2个JSON条目：`palette.json`（palette）或`theme.json`（theme），单文件≤4KiB，整包≤64KiB；最多8个消费令牌，仅`--rv-world-bg`与`--rv-faction-1`…`--rv-faction-7`，颜色必须是不透明`#RRGGBB`。
- **拒绝** shader/WGSL、sprite/纹理、任意路径、脚本、网络src、应用层令牌与透明/非十六进制值。通用`.rview`仍可含着色器，但本私有Arena链不加载、不走naga/执行。
- 精确library依赖最多4直接、8唯一节点、4层，依赖合计512KiB，根另限64KiB；后序解析、root优先覆盖，缺精确依赖/CID/撤销/环均拒绝。只能应用`theme`根。
- 素材仅CC0-1.0或CC-BY-4.0；后者必须非空≤256 UTF8署名。AGPL或其它许可证可存在于通用容器，私有主题发布/加载拒绝。签名/白名单不代表版权背书。
- 权限最小：不改变fog可见性规则、不把隐藏实体送进FrameScene，未知格只用主题背景遮挡真实地形。GPU迷雾叠层仍是背景色加固定0xC0透明度，不是可见性开关。换世界/断连/撤销恢复内置主题；已下载字节不能远程收回，不称DRM。
- Arena/11、CURRENT_11、WAL/2、Lab kernel/4不变；无旧私有视觉格式，不删历史数据。

## 必要直接验证与诚实边界

8个不同直接用例（新增8）：主题解析/越权拒绝2，面板显式应用与换世界隔离2，协议camelCase/七色1，原FrameScene主题色/隐藏实体1（本机llvmpipe软件GPU离线回读未知格背景纳入同一用例，不是独立显卡或浏览器），Rust扫描1，本机认证HTTP→Node SDK签名根/library→私有发现/下载/验签/实际`privateVisualTheme`消费1。后者断言背景`#102040`、阵营1/7打包色、根覆盖library、依赖撤销与owner隔离/本人数据导出元数据。

必要assets/ui/visualization/SDK build、web类型、engine/server必要编译与diff检查通过。**软件GPU是llvmpipe离线回读，不是真实浏览器WebGPU、独立显卡或长期稳定证据。** 不以MemoryStore/文件CAS当PG灾备。无全workspace/Clippy/license/SBOM/soak/CI轮询；无外部发行部署、额外运行时依赖、外采美术、第三方模型或网络浏览器策略绕过。

下一主体优先收拢完整生命成本仍欠项（通用生长/材料体量、EMIT、连续神经/规则计价）；一般TS/rpack/物理ECS、独立Agent赛道/真实LLM、协作presence/离线多主等继续保留。不把本视觉段当全项目完成。
