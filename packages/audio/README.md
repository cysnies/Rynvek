<!-- SPDX-FileCopyrightText: 2026 The Rynvek Contributors -->
<!-- SPDX-License-Identifier: AGPL-3.0-only -->
# 真实Arena音频消费

产品入口：`/arena`连接本人实际实时席位后，点击「手势启用音频」。默认关闭，真实脑态声化需另勾选；主音量/全部静音，断开、换世界/身份、页面隐藏/失焦或3秒无新权威Tick停声，返回后需再次点击。观战/签名回放当前无音频。

`apps/web-platform/src/arena-audio.ts`消费原ArenaClient观察，不发送命令/取钥/拉外部音源。复用AudioDirector、五总线Mixer、冻结程序化synth/recipes与小节调度。本人建筑/出生死亡/科研繁育提示由原真实事件触发，重复Tick及同类冷却不补播；主脑HP下降只表示生命下降，不确定伤害来源。全世界遥测、敌方事件或ghost消失不作声音来源。单位死亡复用现有死亡提示音色，不代表权威爆炸事件。

`NeuralStateSonifier`双低电平载波：
- 原FactionObservation.neural：仅己方LIF/Izhikevich真实64位spike_ring置位数÷神经元数÷64；是各脑最近64个已执行神经子步的重叠窗口，不是每Tick新增spike/瞬时频率。预算冻结与出生使窗口不等同墙钟。
- 己方GatedMLP.hidden_q8/CTRNN.y_q8真实隐藏态绝对均值÷256（展示夹紧），没有脉冲的模型不生成spike。
- 不测或宣称Kuramoto相干度；不发伪spike_cluster/sync_lock事件。音高复用nextBarBoundary、音量短平滑，状态归零则无相应音量。

生命周期先取消输出连接再异步close，迟到resume/旧源不能复活旧图；已交给硬件缓冲/已听到的声音不能撤回。音频错误不改变服务器仿真。无外部资产/Hub依赖，未公共发行；音频包真实制品链、成品采样/音乐分轨与空间化留后续，不把占位配方或空分轨称成品音频。

本轮只做必要类型与定向验证：audio/client tsc build；web-platform tsc --noEmit；`test/arena_audio.test.ts`四项及`test/arena_authored_mount.test.ts`中「实际Arena挂载需显式音频手势」一项。它们用原Rust会话生成的真实跨语言夹具及录制型FakeAudioContext/JSDOM，不是离线DSP渲染或实际浏览器/硬件听验；另有Rust己方投影/原会话/Worker夹具与真实C#本机服务直接验证。命令、边界与接续详见同一`docs/ACTIVE_DEVELOPMENT.md`第三十二批。

## 现行私有PCM与二维空间化

第四十批已将严格PCM16 mono WAV内存采样接入原Director/mixer与原Arena手势生命周期；生成短CC0技术信号用`createTechnicalPcm16Wav`，不是歌曲/第三方音色。私有v2容器/签名/精确依赖/限额、实际可见位置/原相机与DSP未验边界见[PCM_SPATIAL_AUDIO.md](../../docs/PCM_SPATIAL_AUDIO.md)。当前Arena不加载任意网络/通用codec，不声称HRTF或耳机听验。
