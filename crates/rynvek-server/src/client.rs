// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 极简 HTTP/1.1 客户端（仅 `http://`，`Connection: close`，支持 `Content-Length` 与 chunked）。
//!
//! 供 `rynvek` CLI 与集成测试访问本服务端，避免为此引入完整 HTTP 客户端栈与 TLS 依赖。
//! 同时提供挑战 / 签名两步登录的便捷封装。

use rynvek_security::ServerSigningKey;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 响应
#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    /// 头名已转小写
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> Result<serde_json::Value, String> {
        serde_json::from_slice(&self.body).map_err(|e| format!("响应不是 JSON（HTTP {}）：{e}", self.status))
    }

    /// 错误响应中的规范 11.4 错误码
    pub fn error_code(&self) -> Option<u16> {
        self.json().ok()?.get("code")?.as_u64().map(|c| c as u16)
    }
}

/// 响应头上限
const MAX_HEADER_BYTES: usize = 64 << 10;

fn split_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("只支持 http:// 地址：{url}"))?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if host.is_empty() {
        return Err("缺少主机名".to_owned());
    }
    let host = if host.contains(':') {
        host.to_owned()
    } else {
        format!("{host}:80")
    };
    Ok((host, path.to_owned()))
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n").ok_or("chunked 编码截断")?;
        let size_text = std::str::from_utf8(&data[..line_end]).map_err(|_| "chunk 长度非 UTF-8")?;
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| "chunk 长度非法")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            return Err("chunk 数据截断".to_owned());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

/// 发起一次请求
pub async fn request(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<HttpResponse, String> {
    let (host, path) = split_url(url)?;
    let mut stream = tokio::net::TcpStream::connect(&host)
        .await
        .map_err(|e| format!("连接 {host} 失败：{e}"))?;
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(b) = body {
        req.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    req.push_str("\r\n");
    let mut bytes = req.into_bytes();
    if let Some(b) = body {
        bytes.extend_from_slice(b);
    }
    stream.write_all(&bytes).await.map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.map_err(|e| e.to_string())?;
    let head_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .filter(|&i| i <= MAX_HEADER_BYTES)
        .ok_or("响应头不完整")?;
    let head = std::str::from_utf8(&raw[..head_end]).map_err(|_| "响应头非 UTF-8")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("状态行非法：{status_line}"))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let rest = &raw[head_end + 4..];
    let chunked = headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked"));
    let body = if chunked {
        decode_chunked(rest)?
    } else if let Some(len) = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
    {
        rest.get(..len).ok_or("响应体截断")?.to_vec()
    } else {
        rest.to_vec()
    };
    Ok(HttpResponse { status, headers, body })
}

/// GET 便捷函数
pub async fn get(url: &str, token: Option<&str>) -> Result<HttpResponse, String> {
    let auth = token.map(|t| format!("Bearer {t}"));
    let mut h = Vec::new();
    if let Some(a) = &auth {
        h.push(("Authorization", a.as_str()));
    }
    request("GET", url, &h, None).await
}

/// 携带 JSON 体的请求
pub async fn send_json(
    method: &str,
    url: &str,
    token: Option<&str>,
    body: &serde_json::Value,
) -> Result<HttpResponse, String> {
    let auth = token.map(|t| format!("Bearer {t}"));
    let mut h = vec![("Content-Type", "application/json")];
    if let Some(a) = &auth {
        h.push(("Authorization", a.as_str()));
    }
    let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    request(method, url, &h, Some(&bytes)).await
}

/// 登录目标
#[derive(Clone, Copy, Debug)]
pub enum LoginTarget<'a> {
    /// 已注册的 Principal
    Existing(u64),
    /// 首次登录并自注册（`kind`：human / agent）
    Register { handle: &'a str, kind: &'a str },
}

/// 挑战 / 签名两步登录（必要时先自注册），返回 `(principal_id, 文本令牌)`
pub async fn login(base: &str, key: &ServerSigningKey, target: LoginTarget<'_>) -> Result<(u64, String), String> {
    let url = format!("{base}/api/v1/auth/session");
    let req = match target {
        LoginTarget::Existing(pid) => serde_json::json!({ "principal_id": pid.to_string() }),
        LoginTarget::Register { handle, kind } => {
            let pubkey = key.public_key();
            let sig = key.sign(&crate::api::auth::registration_message(handle, &pubkey));
            serde_json::json!({
                "handle": handle,
                "kind": kind,
                "ed25519_pubkey": rynvek_security::to_hex(&pubkey),
                "registration_signature": rynvek_security::to_hex(&sig),
            })
        }
    };
    let r = send_json("POST", &url, None, &req).await?;
    let v = r.json()?;
    if r.status != 200 {
        return Err(format!("取挑战失败（HTTP {}）：{v}", r.status));
    }
    let pid: u64 = v["principal_id"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or("响应缺少 principal_id")?;
    let challenge = v["challenge"].as_str().ok_or("响应缺少 challenge")?;
    let challenge_bytes: [u8; 32] = crate::ids::parse_hex(challenge).ok_or("challenge 非法")?;
    let sig = key.sign(&rynvek_security::challenge_message(pid, &challenge_bytes));
    let r = send_json(
        "POST",
        &url,
        None,
        &serde_json::json!({
            "principal_id": pid.to_string(),
            "challenge": challenge,
            "signature": rynvek_security::to_hex(&sig),
        }),
    )
    .await?;
    let v = r.json()?;
    if r.status != 200 {
        return Err(format!("登录失败（HTTP {}）：{v}", r.status));
    }
    Ok((pid, v["token"].as_str().ok_or("响应缺少 token")?.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_and_urls() {
        assert_eq!(decode_chunked(b"3\r\nabc\r\n2;x\r\nde\r\n0\r\n\r\n").unwrap(), b"abcde");
        assert!(decode_chunked(b"5\r\nab").is_err());
        assert_eq!(
            split_url("http://127.0.0.1:9/api").unwrap(),
            ("127.0.0.1:9".to_owned(), "/api".to_owned())
        );
        assert!(split_url("https://x").is_err());
    }
}
