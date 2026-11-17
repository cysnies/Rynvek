// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";
const root=fileURLToPath(new URL(".",import.meta.url));
// 使用当前开发树源码，不从另一个工作树的旧 dist 冒充本批代码。
const alias=Object.fromEntries(["arena","client","core","protocol","ui","engine","lab"].map(name=>[`@rynvek/${name}`,fileURLToPath(new URL(`../../packages/${name}/src/index.ts`,import.meta.url))]));
export default {
  root,resolve:{alias},worker:{format:"es"},build:{outDir:"web-dist"},
  server:{host:"127.0.0.1",port:5173,strictPort:true,fs:{allow:[fileURLToPath(new URL("../..",import.meta.url)),realpathSync(new URL("../../node_modules",import.meta.url))]},
    proxy:{"/api":{target:process.env.RYNVEK_API ?? "http://127.0.0.1:8787",ws:true}}},
  test:{include:["test/**/*.test.{ts,tsx}"],pool:"forks",maxWorkers:2},
};
