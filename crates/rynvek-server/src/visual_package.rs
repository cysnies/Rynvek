// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! Private .rview profile: eight opaque world/faction colors, zero executable/texture inputs.
use crate::{agent_package::{AgentDependency,credential_free},app::AppState,error::{ApiError,ApiResult},ids::{hex,parse_hex},store::PrivateArtifactRow};
use rynvek_world::package::{read_package,PackageKind,Package};
use serde::{Deserialize,Serialize};
use std::collections::{BTreeMap,BTreeSet};
pub const MAX_ARTIFACT:usize=64<<10;
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {pub schema:String,pub runtime:String,pub role:String,pub credit:String,pub dependencies:Vec<AgentDependency>}
impl Profile {
 pub fn validate(&self)->ApiResult<()> {
  if self.schema!="rynvek.private-visual.v1"||self.runtime!="opaque-palette/1"||!["theme","library"].contains(&self.role.as_str())||self.credit.len()>256||self.credit.chars().any(char::is_control)||self.dependencies.len()>4 {return Err(reject("视觉profile/署名/依赖越界"));}
  credential_free(&self.credit)?;
  let mut previous="";
  for d in &self.dependencies {if d.id.as_str()<=previous||d.id.len()>128||d.version.len()>32||!rynvek_world::package::is_valid_package_id(&d.id)||!rynvek_world::package::is_valid_version(&d.version)||parse_hex::<32>(&d.blake3_256).is_none(){return Err(reject("视觉直接依赖须ID排序、唯一精确版本/CID"));}previous=&d.id;}
  Ok(())
 }
}
/// Canonical typed profile serialization is signed together with raw RYVW digest. Dependencies are NOT unsigned metadata.
pub fn signing_message(owner:u64,id:&str,version:&str,bytes:&[u8],profile:&Profile)->Vec<u8>{
 let mut m=format!("rynvek-private-visual/1\0{owner}\0{id}\0{version}\0").into_bytes();m.extend_from_slice(blake3::hash(bytes).as_bytes());m.extend_from_slice(blake3::hash(&serde_json::to_vec(profile).unwrap()).as_bytes());m
}
pub fn allowed_token(t:&str)->bool { t=="--rv-world-bg"||(1..=7).any(|i|t==format!("--rv-faction-{i}")) }
pub fn parse(bytes:&[u8])->ApiResult<Package>{
 if bytes.len()>MAX_ARTIFACT{return Err(reject("视觉制品超过64KiB"));}
 let p=read_package(bytes,Some(PackageKind::Rview)).map_err(|e|reject(&e.detail))?;
 if p.manifest.engine_compat!="rynvek-private-theme/1"||p.manifest.id.len()>128||p.manifest.version.len()>32||p.manifest.name.len()>128||p.manifest.entries.is_empty()||p.manifest.entries.len()>2{return Err(reject("须受限视觉profile，1–2个JSON条目"));}
 credential_free(&serde_json::to_string(&p.manifest).unwrap())?;
 for e in &p.manifest.entries {
  let key=match (e.path.as_str(),e.kind.as_str(),e.media_type.as_str()){("palette.json","palette","application/json")=>"colors",("theme.json","theme","application/json")=>"tokens",_=>return Err(reject("视觉只许palette.json/theme.json，无shader/脚本/纹理"))};
  let data=p.file(&e.path).unwrap();if data.len()>4096{return Err(reject("视觉JSON超过4KiB"));}
  let text=std::str::from_utf8(data).map_err(|_|reject("视觉UTF8非法"))?;credential_free(text)?;
  let v:serde_json::Value=serde_json::from_str(text).map_err(|_|reject("视觉JSON非法"))?;
  let o=v.as_object().ok_or_else(||reject("视觉JSON须对象"))?;
  if o.len()!=1{return Err(reject("视觉JSON未知字段"));}
  let tokens=v[key].as_object().ok_or_else(||reject("视觉缺colors/tokens"))?;
  if tokens.len()>8{return Err(reject("视觉只许8种消费令牌"));}
  for (k,v) in tokens {let token=if key=="colors"{format!("--rv-{k}")}else{k.clone()};let c=v.as_str().ok_or_else(||reject("视觉颜色须字符串"))?;if !allowed_token(&token)||c.len()!=7||!c.starts_with('#')||!c.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit){return Err(reject("视觉只许世界背景/七阵营不透明#RRGGBB颜色"));}}
 }
 Ok(p)
}
pub fn validate_license(p:&Package,profile:&Profile)->ApiResult<()> {
 if !matches!(p.manifest.license.as_str(),"CC0-1.0"|"CC-BY-4.0"){return Err(reject("私有视觉只许CC0/CC-BY，不含可执行着色器许可包"));}
 if p.manifest.license=="CC-BY-4.0"&&profile.credit.trim().is_empty(){return Err(reject("CC-BY视觉必须署名"));}Ok(())
}
pub fn load_artifact(app:&AppState,row:&PrivateArtifactRow)->ApiResult<Vec<u8>> {
 if row.revoked{return Err(reject("视觉版本已撤销"));}
 let profile:Profile=serde_json::from_value(row.profile.clone()).map_err(|_|reject("视觉profile损坏"))?;profile.validate()?;
 let bytes=app.inner.world_snapshots.get(&rynvek_core::Blake3Hash(row.cid))?;
 if bytes.len()>MAX_ARTIFACT||blake3::hash(&bytes).as_bytes()!=&row.cid{return Err(reject("视觉CAS不完整"));}
 rynvek_security::verify_ed25519(&row.publisher_key,&signing_message(row.owner_id,&row.package_id,&row.version,&bytes,&profile),&row.signature).map_err(ApiError::from)?;
 let p=parse(&bytes)?;validate_license(&p,&profile)?;if p.manifest.id!=row.package_id||p.manifest.version!=row.version{return Err(reject("视觉元数据身份不符"));}Ok(bytes)
}
/// Bounded postorder DFS: all reachable exact dependencies must be same-owner libraries; each applied once, root last.
pub async fn resolve(app:&AppState,owner:u64,id:&str,version:&str,root:Profile)->ApiResult<Vec<PrivateArtifactRow>> {
 let root_key=format!("{id}@{version}");let mut todo=vec![(root_key.clone(),root,0usize,false)];let mut path=BTreeSet::new();let mut done=BTreeSet::new();let mut rows:BTreeMap<String,PrivateArtifactRow>=BTreeMap::new();let mut pins=Vec::new();let mut total=0usize;
 while let Some((key,p,depth,exit))=todo.pop(){
  if exit {path.remove(&key);if done.insert(key.clone())&&key!=root_key{pins.push(rows.get(&key).unwrap().clone());}continue;}
  if depth>4||!path.insert(key.clone()){return Err(reject("视觉依赖环/超过4层"));}if done.contains(&key){path.remove(&key);continue;}
  let deps=p.dependencies.clone();todo.push((key,p,depth,true));
  for d in deps.into_iter().rev(){let k=format!("{}@{}",d.id,d.version);let r=app.store().private_visual(owner,d.id.clone(),d.version.clone()).await?.ok_or_else(||reject("缺少本人精确视觉依赖"))?;
   if hex(&r.cid)!=d.blake3_256{return Err(reject("视觉依赖CID不符"));}
   let bytes=load_artifact(app,&r)?;let p:Profile=serde_json::from_value(r.profile.clone()).map_err(|_|reject("视觉依赖profile非法"))?;if p.role!="library"{return Err(reject("视觉依赖只能library"));}
   if !rows.contains_key(&k){total+=bytes.len();if rows.len()>=8||total>512<<10{return Err(reject("视觉依赖图资源超限"));}rows.insert(k.clone(),r);}
   todo.push((k,p,depth+1,false));
  }
 }
 Ok(pins)
}
pub use crate::audio_package::dependencies;

pub use crate::agent_package::reject;
