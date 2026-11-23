// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { useEffect, useRef, useState } from "react";
import { createGeneralRenderHost } from "@rynvek/engine";
import type { GeneralWorldSession } from "./general-remote.js";
/** 显式开启只读显示。5秒复验、12秒失鲜即清除；没有后台Tick/本地积分或假Canvas降级。 */
export function GeneralViewport({ session, onInvalidated }: {
    session: GeneralWorldSession;
    onInvalidated: () => void;
}) {
    const [enabled, setEnabled] = useState(false), [status, setStatus] = useState("未开启GPU只读显示"), canvas = useRef<HTMLCanvasElement>(null);
    const callback = useRef(onInvalidated);
    callback.current = onInvalidated;
    useEffect(() => {
        if (!enabled || !canvas.current || !session.world)
            return;
        const element = canvas.current, abort = new AbortController();
        let disposed = false, render: ReturnType<typeof createGeneralRenderHost> | undefined, poll: ReturnType<typeof setTimeout> | undefined, expiry: ReturnType<typeof setTimeout> | undefined, startup: ReturnType<typeof setTimeout> | undefined;
        const stop = (message: string, invalidate = false) => { if (disposed)
            return; disposed = true; element.hidden = true; abort.abort(); clearTimeout(poll); clearTimeout(expiry); clearTimeout(startup); render?.dispose(); setStatus(message); setEnabled(false); if (invalidate) {
            session.invalidatePrivate();
            callback.current();
        } };
        const off = session.onPrivateInvalidated(() => stop("身份/世界/观察失效，私有画布已清除"));
        const offline = () => stop("网络离线，私有画布已清除", true), hidden = () => { if (document.hidden)
            stop("页面隐藏，私有画布已清除", true); };
        window.addEventListener("offline", offline);
        document.addEventListener("visibilitychange", hidden);
        async function read() { try {
            const f = await session.renderFrame(abort.signal);
            if (disposed)
                return;
            render!.present(f);
            clearTimeout(expiry);
            expiry = setTimeout(() => stop("授权观察超过12秒未复验，私有画布已清除", true), 12000);
            poll = setTimeout(() => void read(), 5000);
        }
        catch {
            if (!disposed)
                stop("观察请求/授权/帧失效，私有画布已清除；请明确刷新重连", true);
        } }
        try {
            if (typeof element.transferControlToOffscreen !== "function")
                throw new Error("OffscreenCanvas不可用");
            setStatus("加载原WASM/Worker，等待真实GPU及General能力握手");
            render = createGeneralRenderHost(new Worker(new URL("./engine.worker.ts", import.meta.url), { type: "module", name: "rynvek-general-render" }), element, session.world.replaceAll("-", ""), m => { if (!disposed) {
                clearTimeout(startup);
                setStatus(m);
            } }, () => stop("Worker/GPU未确认显示，私有视图已清除；没有自动Canvas替身", true));
            startup = setTimeout(() => stop("Worker/授权就绪超时，私有画布已清除", true), 12000);
            void read();
        }
        catch {
            stop("当前环境或Worker不支持真实显示，私有视图已清除，未宣称成功", true);
        }
        return () => { off(); window.removeEventListener("offline", offline); document.removeEventListener("visibilitychange", hidden); if (!disposed) {
            disposed = true;
            element.hidden = true;
            abort.abort();
            clearTimeout(poll);
            clearTimeout(expiry);
            clearTimeout(startup);
            render?.dispose();
        } };
    }, [enabled, session, session.world]);
    return <section><h2>原Worker · 权威General只读显示</h2><p>只展示服务器已提交位置/半径，5秒只读复验，无本地推演。撤权在下次复验失败清除，最迟12秒失鲜清除；离线/隐藏/换世界立即清除，不承诺撤回已见信息。</p><button disabled={!session.world || enabled} onClick={() => setEnabled(true)}>明确开启WebGPU只读显示</button><button disabled={!enabled} onClick={() => { setStatus("已停止并清画布"); setEnabled(false); }}>停止并清画布</button><p role="status">{status}</p>{enabled && <canvas ref={canvas} aria-label="General权威只读画布"/>}</section>;
}
