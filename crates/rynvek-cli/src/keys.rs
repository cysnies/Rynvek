// SPDX-FileCopyrightText: 2026 The Rynvek Contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 种子文件：一行 64 位小写十六进制（32 字节 Ed25519 种子）+ 换行。
//!
//! 文件以 `create_new` 创建，绝不覆盖已有密钥；Unix 上权限为 `0600`。

use std::io::Write;
use std::path::Path;

use rynvek_security::ServerSigningKey;

use crate::{CliError, CliResult};

/// 生成并写入种子，打印公钥
pub fn keygen(out: &Path) -> CliResult {
    let key = ServerSigningKey::generate(0).map_err(|e| CliError::new(e.to_string()))?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(out)
        .map_err(|e| CliError::new(format!("无法创建 {}：{e}", out.display())))?;
    writeln!(f, "{}", rynvek_security::to_hex(&key.seed())).map_err(|e| CliError::new(e.to_string()))?;
    crate::print_json(&serde_json::json!({
        "key_file": out.display().to_string(),
        "ed25519_public_key": rynvek_security::to_hex(&key.public_key()),
    }));
    Ok(())
}

/// 读取种子文件
pub fn load(path: &Path, key_id: u16) -> CliResult<ServerSigningKey> {
    let text = std::fs::read_to_string(path).map_err(|e| CliError::new(format!("无法读取 {}：{e}", path.display())))?;
    let seed: [u8; 32] = rynvek_security::from_hex(text.trim())
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| CliError::new(format!("{} 不是 32 字节十六进制种子", path.display())))?;
    Ok(ServerSigningKey::from_seed(key_id, seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keygen_round_trips_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("k.seed");
        keygen(&p).unwrap();
        let k = load(&p, 3).unwrap();
        assert_eq!(k.key_id(), 3);
        assert!(keygen(&p).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::write(&p, "zz").unwrap();
        assert!(load(&p, 0).is_err());
    }
}
