// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// Real loopback service; renderer color math is the actual Arena theme function. No credentials in output.
import assert from 'node:assert/strict';
import {createPrivateKey,sign} from 'node:crypto';
import {readFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {resolve} from 'node:path';
import ts from 'typescript';
import {PrivateAgentHubClient,signPrivateVisualPackage,agentDigest} from '../dist/index.js';
import {writeRview} from '../../assets/dist/index.js';
let raw='';for await(const chunk of process.stdin)raw+=chunk;const c=JSON.parse(raw);raw='';
const alarm=setTimeout(()=>{console.error('视觉真实消费快验超时');process.exit(1);},15000);
const hub=new PrivateAgentHubClient(c.base,c.principalId,()=>c.token),sdk=hub.visual();
try {
 const key=createPrivateKey({key:Buffer.from('302e020100300506032b657004220420'+c.testSeed.toString(16).padStart(2,'0').repeat(32),'hex'),format:'der',type:'pkcs8'});
 const signer=async message=>new Uint8Array(sign(null,message,key));
 const enc=new TextEncoder();
 const entry=(path,kind,value)=>({path,kind,media_type:'application/json',data:enc.encode(JSON.stringify(value))});
 const rview=(id,version,entries)=>writeRview({id,version,name:id,license:'CC0-1.0',engine_compat:'rynvek-private-theme/1',entries});
 const libBytes=rview('owner.visual-library','1.0.0',[entry('palette.json','palette',{colors:{'world-bg':'#000000','faction-7':'#abcdef'}})]);
 const libProfile={schema:'rynvek.private-visual.v1',runtime:'opaque-palette/1',role:'library',credit:'',dependencies:[]};
 const lib=await sdk.publish(await signPrivateVisualPackage(c.principalId,'owner.visual-library','1.0.0',libBytes,libProfile,signer));
 const rootBytes=rview('owner.visual-main','1.0.0',[entry('theme.json','theme',{tokens:{
  '--rv-world-bg':'#102040','--rv-faction-1':'#e01080','--rv-faction-2':'#20c080','--rv-faction-3':'#ff7f00',
  '--rv-faction-4':'#00ff7f','--rv-faction-5':'#7f00ff','--rv-faction-6':'#00ffff','--rv-faction-7':'#010203'
 }})]);
 const rootProfile={schema:'rynvek.private-visual.v1',runtime:'opaque-palette/1',role:'theme',credit:'Rynvek self-generated CC0 test palette',dependencies:[{id:lib.package_id,version:lib.version,blake3_256:lib.artifact_cid}]};
 const upload=await signPrivateVisualPackage(c.principalId,'owner.visual-main','1.0.0',rootBytes,rootProfile,signer);
 const root=await sdk.publish(upload);
 await assert.rejects(()=>sdk.publish(upload));
 const found=await sdk.list();assert.equal(found.length,2);
 const loaded=await sdk.load(found.find(m=>m.profile.role==='theme'));
 await assert.rejects(()=>sdk.download({...root,signature:'00'.repeat(64)}));
 await assert.rejects(()=>sdk.download({...root,artifact_cid:'00'.repeat(32)}));
 const downloaded=await sdk.download(root);assert.equal(agentDigest(downloaded),root.artifact_cid);
 const source=readFileSync(resolve('apps/web-platform/src/arena-visual.ts'),'utf8');
 const rewrite=(text,name,file)=>text.replaceAll(JSON.stringify(name),JSON.stringify(pathToFileURL(resolve(file)).href));
 let js=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText;
 js=rewrite(js,'@rynvek/assets','packages/assets/dist/index.js');
 js=rewrite(js,'@rynvek/visualization','packages/visualization/dist/index.js');
 js=rewrite(js,'@rynvek/ui','packages/ui/dist/index.js');
 js=rewrite(js,'@rynvek/agent-sdk','packages/agent-sdk/dist/index.js');
 const {privateVisualTheme}=await import('data:text/javascript;base64,'+Buffer.from(js).toString('base64'));
 const theme=privateVisualTheme(loaded);
 assert.equal(theme.background,0xFF402010);
 assert.equal(theme.factions[0],0xFF8010E0);
 assert.equal(theme.factions[6],0xFF030201,'根包必须覆盖library的faction-7');
 await sdk.revoke(lib);await assert.rejects(()=>sdk.load(root));
 const newBytes=rview('owner.visual-main','1.0.1',[entry('theme.json','theme',{tokens:{'--rv-world-bg':'#102040'}})]);
 const newProfile={...rootProfile,dependencies:[{id:lib.package_id,version:lib.version,blake3_256:lib.artifact_cid}]};
 await assert.rejects(async()=>sdk.publish(await signPrivateVisualPackage(c.principalId,'owner.visual-main','1.0.1',newBytes,newProfile,signer)));
 await sdk.revoke(root);await assert.rejects(()=>sdk.download({...root,revoked:false}));
 console.log(JSON.stringify({root,lib,downloaded:true,background:theme.background,factions:theme.factions,libraryRevocationEnforced:true}));
} finally {hub.destroy();c.token='';clearTimeout(alarm);}
