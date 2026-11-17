// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only
//! Signed private PCM16/procedural banks: bounded data, no JS/platform codecs/URLs.
use crate::{
    agent_package::{AgentDependency, AgentLock, credential_free},
    app::AppState,
    error::{ApiError, ApiResult},
    ids::{hex, parse_hex},
    store::PrivateArtifactRow,
};
use rynvek_world::package::{PackageKind, read_package};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_ARTIFACT: usize = 512 << 10;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub runtime: String,
    pub role: String,
}
#[derive(Clone, Debug)]
pub struct AudioPackage {
    pub id: String,
    pub version: String,
    pub profile: Profile,
    pub manifest: Value,
    pub recipes: Value,
    pub lock: AgentLock,
    pub samples: BTreeMap<String, Vec<u8>>,
}
pub fn signing_message(owner: u64, id: &str, version: &str, bytes: &[u8]) -> Vec<u8> {
    let mut m = format!("rynvek-private-audio/1\0{owner}\0{id}\0{version}\0").into_bytes();
    m.extend_from_slice(blake3::hash(bytes).as_bytes());
    m
}
fn obj<'a>(v: &'a Value, keys: &[&str]) -> ApiResult<&'a serde_json::Map<String, Value>> {
    let m = v.as_object().ok_or_else(|| reject("音频数据须为对象"))?;
    if m.keys().any(|k| !keys.contains(&k.as_str())) {
        return Err(reject("音频数据含未知字段"));
    }
    Ok(m)
}
fn num(v: &Value, lo: f64, hi: f64, integer: bool) -> ApiResult<()> {
    let n = v.as_f64().ok_or_else(|| reject("音频参数须为有限数字"))?;
    if !n.is_finite() || n < lo || n > hi || (integer && n.fract() != 0.) {
        return Err(reject("音频参数越界"));
    }
    Ok(())
}
// Frozen recipe event/bus mapping; both Rust scanning and the TS consumer enforce it.
pub fn bus(id: &str) -> Option<&'static str> {
    Some(match id {
        "ui.hover" | "ui.click" | "ui.confirm" | "ui.cancel" | "ui.panel_toggle" | "unit.selected"
        | "unit.order_move" | "unit.order_attack" => "sfx_ui",
        "ui.error" | "alert.nexus_under_attack" => "voice_alert",
        "building.placed"
        | "building.completed"
        | "building.extractor_cycle"
        | "unit.spawned"
        | "neural.mutation_epoch"
        | "field.beacon_pulse" => "sfx_world",
        "building.destroyed"
        | "combat.weapon_pulse"
        | "combat.spore_launch"
        | "combat.impact_kinetic"
        | "combat.shield_hit"
        | "combat.explosion_large" => "sfx_combat",
        "neural.spike_cluster" | "neural.sync_lock" => "neural_sonify",
        _ => return None,
    })
}
/// Canonical uncompressed PCM16 mono RIFF only; no platform codecs or metadata chunks.
pub fn pcm_frames(b: &[u8]) -> ApiResult<usize> {
    if b.len() < 46 || b.len() > 192_044 {
        return Err(reject("PCM尺寸越界"));
    }
    let u16at = |i| u16::from_le_bytes(b[i..i + 2].try_into().unwrap());
    let u32at = |i| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let rate = u32at(24);
    if &b[..4] != b"RIFF"
        || &b[8..16] != b"WAVEfmt "
        || &b[36..40] != b"data"
        || u32at(4) as usize != b.len() - 8
        || u32at(16) != 16
        || u16at(20) != 1
        || u16at(22) != 1
        || ![8000, 16000, 22050, 24000, 44100, 48000].contains(&rate)
        || u32at(28) != rate * 2
        || u16at(32) != 2
        || u16at(34) != 16
        || u32at(40) as usize != b.len() - 44
        || (b.len() - 44) % 2 != 0
        || (b.len() - 44) / 2 > rate as usize * 2
    {
        return Err(reject("只支持最多2秒规范PCM16 mono WAV，无额外块"));
    }
    Ok((b.len() - 44) / 2)
}
fn sample_path(path: &str) -> bool {
    path.strip_prefix("samples/")
        .and_then(|s| s.strip_suffix(".wav"))
        .is_some_and(|s| parse_hex::<32>(s).is_some())
}
pub fn validate_manifest(v: &Value) -> ApiResult<()> {
    obj(v, &["schema", "events"])?;
    if v["schema"] != "rynvek.audio-manifest.v1" {
        return Err(reject("音频manifest版本非法"));
    }
    let events = v["events"].as_object().ok_or_else(|| reject("缺少events"))?;
    if events.len() > 24 {
        return Err(reject("最多24类既有音频事件"));
    }
    for (id, e) in events {
        obj(
            e,
            &[
                "status",
                "bus",
                "gain",
                "cooldown_ms",
                "max_voices",
                "src",
                "license",
                "credit",
            ],
        )?;
        if !["placeholder", "asset"].contains(&e["status"].as_str().unwrap_or(""))
            || e["bus"].as_str() != bus(id)
            || bus(id).is_none()
        {
            return Err(reject("音频制品仅允许规范事件及其原总线"));
        }
        if e["status"] == "asset" {
            if !e["src"].as_str().is_some_and(sample_path)
                || !["CC0-1.0", "CC-BY-4.0"].contains(&e["license"].as_str().unwrap_or(""))
                || e.get("credit")
                    .is_some_and(|c| c.as_str().is_none_or(|s| s.trim().is_empty() || s.len() > 256))
                || (e["license"] == "CC-BY-4.0" && e.get("credit").is_none())
            {
                return Err(reject("采样路径/许可证/署名非法"));
            }
        } else if ["src", "license", "credit"].iter().any(|k| e.get(k).is_some()) {
            return Err(reject("占位事件不可含媒体字段"));
        }
        for (k, lo, hi, int) in [
            ("gain", 0., 1., false),
            ("cooldown_ms", 0., 60000., false),
            ("max_voices", 1., 16., true),
        ] {
            if let Some(v) = e.get(k) {
                num(v, lo, hi, int)?;
            }
        }
    }
    Ok(())
}
pub fn validate_recipes(v: &Value) -> ApiResult<()> {
    let recipes = v.as_object().ok_or_else(|| reject("recipes须为对象"))?;
    if recipes.len() > 24 {
        return Err(reject("最多24个配方覆写"));
    }
    for (id, p) in recipes {
        if bus(id).is_none() {
            return Err(reject("未知程序化配方"));
        }
        let m = obj(
            p,
            &[
                "oscType",
                "freqStart",
                "freqEnd",
                "attackMs",
                "decayMs",
                "sustainLevel",
                "releaseMs",
                "filterType",
                "filterFreq",
                "filterQ",
                "noiseMix",
            ],
        )?;
        for (k, v) in m {
            match k.as_str() {
                "oscType" => {
                    if !["sine", "square", "sawtooth", "triangle"].contains(&v.as_str().unwrap_or("")) {
                        return Err(reject("振荡器类型非法"));
                    }
                }
                "filterType" => {
                    if !["lowpass", "highpass", "bandpass"].contains(&v.as_str().unwrap_or("")) {
                        return Err(reject("滤波器类型非法"));
                    }
                }
                "freqStart" | "freqEnd" | "filterFreq" => num(v, 20., 8000., false)?,
                "attackMs" => num(v, 1., 50., false)?,
                "decayMs" | "releaseMs" => num(v, 1., 500., false)?,
                "filterQ" => num(v, 0.1, 5., false)?,
                _ => num(v, 0., if k == "noiseMix" { 0.8 } else { 1. }, false)?,
            }
        }
    }
    Ok(())
}
pub fn parse(bytes: &[u8]) -> ApiResult<AudioPackage> {
    if bytes.is_empty() || bytes.len() > MAX_ARTIFACT {
        return Err(reject("音频制品必须1..512KiB"));
    }
    let p = read_package(bytes, Some(PackageKind::Rpack)).map_err(|e| ApiError::new(e.code, e.detail))?;
    if p.manifest.id.len() > 128
        || p.manifest.version.len() > 32
        || p.manifest.name.len() > 128
        || p.manifest.engine_compat != "rynvek-audio-pcm16/2"
        || !(4..=12).contains(&p.manifest.entries.len())
    {
        return Err(reject("须为当前audio-pcm16/2数据RYPK"));
    }
    for path in ["audio.json", "manifest.json", "recipes.json", "dependencies.lock"] {
        if !p
            .manifest
            .entries
            .iter()
            .any(|e| e.path == path && e.kind == "data" && e.media_type == "application/json")
        {
            return Err(reject("音频制品须含四个规范JSON，仅另允许受限PCM采样"));
        }
    }
    credential_free(&serde_json::to_string(&p.manifest).unwrap())?;
    let mut samples = BTreeMap::new();
    let mut frames = 0;
    for entry in &p.manifest.entries {
        let data = &p.files[&entry.path];
        if sample_path(&entry.path) {
            if entry.kind != "data"
                || entry.media_type != "application/octet-stream"
                || entry.path != format!("samples/{}.wav", hex(blake3::hash(data).as_bytes()))
            {
                return Err(reject("采样类型/内容地址不符"));
            }
            frames += pcm_frames(data)?;
            if samples.len() >= 8 || frames > 192_000 {
                return Err(reject("采样帧数/数量超限"));
            }
            samples.insert(entry.path.clone(), data.clone());
            continue;
        }
        if !["audio.json", "manifest.json", "recipes.json", "dependencies.lock"].contains(&entry.path.as_str()) {
            return Err(reject("不允许额外条目"));
        }
        let s = std::str::from_utf8(data).map_err(|_| reject("音频数据须为UTF8"))?;
        credential_free(s)?;
    }
    let profile: Profile =
        serde_json::from_slice(p.file("audio.json").unwrap()).map_err(|_| reject("音频profile非法"))?;
    if profile.schema != "rynvek.audio-pack.v2"
        || profile.runtime != "pcm16-recipes/2"
        || !["soundpack", "library"].contains(&profile.role.as_str())
    {
        return Err(reject("不支持的音频profile"));
    }
    let manifest: Value =
        serde_json::from_slice(p.file("manifest.json").unwrap()).map_err(|_| reject("音频manifest非法"))?;
    validate_manifest(&manifest)?;
    let recipes: Value = serde_json::from_slice(p.file("recipes.json").unwrap()).map_err(|_| reject("recipes非法"))?;
    validate_recipes(&recipes)?;
    let lock: AgentLock =
        serde_json::from_slice(p.file("dependencies.lock").unwrap()).map_err(|_| reject("音频精确依赖锁非法"))?;
    let mut ids = BTreeSet::new();
    if lock.lockfile_version != 1 || lock.packages.len() > 4 {
        return Err(reject("音频依赖最多4个直接库"));
    }
    for d in &lock.packages {
        if !ids.insert(&d.id)
            || d.id.len() > 128
            || d.version.len() > 32
            || !rynvek_world::package::is_valid_package_id(&d.id)
            || !rynvek_world::package::is_valid_version(&d.version)
            || parse_hex::<32>(&d.blake3_256).is_none()
        {
            return Err(reject("音频依赖须为唯一精确ID/版本/摘要"));
        }
    }
    Ok(AudioPackage {
        id: p.manifest.id,
        version: p.manifest.version,
        profile,
        manifest,
        recipes,
        lock,
        samples,
    })
}
pub fn load_artifact(app: &AppState, row: &PrivateArtifactRow) -> ApiResult<Vec<u8>> {
    if row.revoked {
        return Err(reject("音频版本已撤销"));
    }
    let bytes = app.inner.world_snapshots.get(&rynvek_core::Blake3Hash(row.cid))?;
    if bytes.len() > MAX_ARTIFACT || blake3::hash(&bytes).as_bytes() != &row.cid {
        return Err(reject("私有音频CAS不完整"));
    }
    rynvek_security::verify_ed25519(
        &row.publisher_key,
        &signing_message(row.owner_id, &row.package_id, &row.version, &bytes),
        &row.signature,
    )
    .map_err(ApiError::from)?;
    let p = parse(&bytes)?;
    if p.id != row.package_id || p.version != row.version || serde_json::to_value(p.profile).unwrap() != row.profile {
        return Err(reject("音频元数据不一致"));
    }
    Ok(bytes)
}
#[derive(Debug)]
pub struct ResolvedAudio {
    pub pins: Vec<PrivateArtifactRow>,
    pub manifest: Value,
    pub recipes: Value,
    pub samples: BTreeMap<String, Vec<u8>>,
}
// Bounded DFS; postorder + lock order, then root wins. Diamond nodes applied once.
pub async fn resolve(app: &AppState, owner: u64, root: AudioPackage) -> ApiResult<ResolvedAudio> {
    let mut todo = vec![(root, 0usize, false)];
    let mut path = BTreeSet::new();
    let mut done = BTreeSet::new();
    let mut cids = BTreeMap::new();
    let mut pins = Vec::new();
    let mut events = serde_json::Map::new();
    let mut recipes = serde_json::Map::new();
    let mut total = 0;
    let mut samples = BTreeMap::new();
    while let Some((p, depth, exit)) = todo.pop() {
        let key = format!("{}@{}", p.id, p.version);
        if exit {
            for (key, bytes) in &p.samples {
                samples.insert(key.clone(), bytes.clone());
            }
            if samples.len() > 8
                || samples
                    .values()
                    .map(|b| pcm_frames(b))
                    .collect::<ApiResult<Vec<_>>>()?
                    .iter()
                    .sum::<usize>()
                    > 192_000
            {
                return Err(reject("解析后采样资源超限"));
            }
            path.remove(&key);
            if !done.insert(key) {
                continue;
            }
            for (k, v) in p.manifest["events"].as_object().unwrap() {
                events.insert(k.clone(), v.clone());
            }
            for (k, v) in p.recipes.as_object().unwrap() {
                let entry = recipes.entry(k.clone()).or_insert_with(|| json!({}));
                entry.as_object_mut().unwrap().extend(v.as_object().unwrap().clone());
            }
            continue;
        }
        if depth > 4 || !path.insert(key.clone()) {
            return Err(reject("音频依赖环/超过4层"));
        }
        if done.contains(&key) {
            path.remove(&key);
            continue;
        }
        let deps = p.lock.packages.clone();
        todo.push((p, depth, true));
        for d in deps.into_iter().rev() {
            let key = format!("{}@{}", d.id, d.version);
            if cids
                .insert(key, d.blake3_256.clone())
                .is_some_and(|old| old != d.blake3_256)
            {
                return Err(reject("音频依赖图CID冲突"));
            }
            let row = app
                .store()
                .private_audio(owner, d.id.clone(), d.version.clone())
                .await?
                .ok_or_else(|| reject("缺少本人精确音频依赖"))?;
            if hex(&row.cid) != d.blake3_256 {
                return Err(reject("音频依赖CID不符"));
            }
            let bytes = load_artifact(app, &row)?;
            let child = parse(&bytes)?;
            if child.profile.role != "library" {
                return Err(reject("音频依赖只能是library"));
            }
            if !pins
                .iter()
                .any(|p: &PrivateArtifactRow| p.package_id == d.id && p.version == d.version)
            {
                total += bytes.len();
                if pins.len() >= 8 || total > 4 << 20 {
                    return Err(reject("音频依赖图资源超限"));
                }
                pins.push(row);
            }
            todo.push((child, depth + 1, false));
        }
    }
    for event in events.values() {
        if event["status"] == "asset" && !event["src"].as_str().is_some_and(|path| samples.contains_key(path)) {
            return Err(reject("精确依赖图缺少采样"));
        }
    }
    Ok(ResolvedAudio {
        pins,
        samples,
        manifest: json!({"schema":"rynvek.audio-manifest.v1","events":events}),
        recipes: Value::Object(recipes),
    })
}
pub fn dependencies(pins: &[PrivateArtifactRow]) -> Vec<AgentDependency> {
    pins.iter()
        .map(|r| AgentDependency {
            id: r.package_id.clone(),
            version: r.version.clone(),
            blake3_256: hex(&r.cid),
        })
        .collect()
}

pub use crate::agent_package::reject;
