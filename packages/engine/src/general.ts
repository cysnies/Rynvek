// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { EngineHost, type WorkerLike, type TransferableCanvasLike } from "./host.js";
import type { GeneralRenderFrame } from "./protocol.js";
export function validateGeneralFrame(f: GeneralRenderFrame, world: string): void {
    const keys = (v: object, k: string) => Object.keys(v).sort().join(",") === k;
    if (!f || !keys(f, "bodies,format,revision,stateRoot,tick,worldId") || f.format !== "rynvek-general-frame/1" || f.worldId !== world || !/^[0-9a-f]{32}$/.test(world) || !Number.isSafeInteger(f.tick) || f.tick < 0 || f.tick !== f.revision || !/^[0-9a-f]{64}$/.test(f.stateRoot) || !Array.isArray(f.bodies) || f.bodies.length > 128)
        throw new Error("General帧身份/根/字段/范围非法");
    let serial = 0;
    const one = 4294967296;
    for (const b of f.bodies) {
        if (!b || !keys(b, "positionQ32,radiusQ32,serial") || !Number.isInteger(b.serial) || b.serial <= serial || b.serial >= 0xffffffff || !Number.isSafeInteger(b.radiusQ32) || b.radiusQ32 < one / 4 || b.radiusQ32 > 2 * one || !Array.isArray(b.positionQ32) || b.positionQ32.length !== 2 || b.positionQ32.some(v => !Number.isSafeInteger(v) || v < b.radiusQ32 || v > 256 * one - b.radiusQ32))
            throw new Error("General圆体投影非法");
        serial = b.serial;
    }
}
/** 沿原Dedicated Worker/EngineHost/Renderer，不接收令牌、规则源码或完整私有观察。 */
export function createGeneralRenderHost(worker: WorkerLike, canvas: TransferableCanvasLike, worldId: string, onStatus: (message: string) => void, onError: (error: Error) => void) {
    const host = new EngineHost(worker);
    const sent = new Map<number, string>();
    let disposed = false, applied: {
        tick: number;
        root: string;
    } | undefined;
    const fail = (e: Error) => { if (disposed)
        return; dispose(); onError(e); };
    const offReady = host.on("ENGINE_READY", e => { if (e.generalRemote !== true || e.backend !== "webgpu")
        fail(new Error("Worker未确认General只读GPU能力")); });
    const offError = host.on("ENGINE_ERROR", e => fail(new Error(e.message))), offDegraded = host.on("BACKEND_DEGRADED", e => fail(new Error(e.reason)));
    const offApplied = host.on("GENERAL_FRAME_APPLIED", e => { if (e.worldId !== worldId || sent.get(e.tick) !== e.stateRoot) {
        fail(new Error("观察回执世界不匹配"));
        return;
    } applied = { tick: e.tick, root: e.stateRoot }; onStatus(`Worker已接收权威Tick ${e.tick}；待GPU提交帧`); });
    const offRendered = host.on("FRAME_RENDERED", e => { if (applied && e.tick === applied.tick)
        onStatus(`WebGPU已提交权威Tick ${e.tick} · 根 ${applied.root}（非浏览器硬件验收）`); });
    function dispose() { if (disposed)
        return; disposed = true; applied = undefined; sent.clear(); offReady(); offError(); offDegraded(); offApplied(); offRendered(); host.shutdown(); }
    try {
        if (!/^[0-9a-f]{32}$/.test(worldId))
            throw new Error("世界绑定非法");
        host.attach(canvas, { width: 768, height: 768, devicePixelRatio: 1, worldId, gpuMode: "required", generalRemote: true });
        host.send({ type: "SET_CAMERA", centerX: 128, centerY: 128, pxPerCell: 3 });
    }
    catch (e) {
        dispose();
        throw e;
    }
    return { host, present(f: GeneralRenderFrame) { if (disposed)
            throw new Error("只读画布已清除"); validateGeneralFrame(f, worldId); sent.set(f.tick, f.stateRoot); if (sent.size > 4)
            sent.delete(sent.keys().next().value!); host.send({ type: "GENERAL_FRAME", frame: { format: f.format, worldId: f.worldId, tick: f.tick, revision: f.revision, stateRoot: f.stateRoot, bodies: f.bodies.map(b => ({ serial: b.serial, positionQ32: [...b.positionQ32], radiusQ32: b.radiusQ32 })) } }); }, dispose };
}
