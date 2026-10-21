//! Canonical content JSON v1: sorted keys, array order preserved, six decimal grid.
//! Restrict numbers to +/-1e6 and exact multiples of 1e-6 (within 1e-8 scaled tolerance).
//! This deliberately avoids cross-language exponent/float formatting ambiguity.
use crate::Result;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
pub fn number(n: f64) -> Result<String> {
    if !n.is_finite()
        || n.abs() > 1_000_000.0
        || (n * 1_000_000.0 - (n * 1_000_000.0).round()).abs() > 0.0001
    {
        return Err("content number outside canonical six-decimal grid".into());
    }
    if n == 0.0 {
        return Ok("0".into());
    }
    let s = format!("{n:.6}");
    Ok(s.trim_end_matches('0').trim_end_matches('.').to_owned())
}
pub fn bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    let mut output = String::new();
    fn write(value: &Value, out: &mut String) -> Result<()> {
        match value {
            Value::Number(n) => out.push_str(&number(n.as_f64().ok_or("invalid content number")?)?),
            Value::Array(values) => {
                out.push('[');
                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(value, out)?;
                }
                out.push(']');
            }
            Value::Object(values) => {
                out.push('{');
                let mut keys: Vec<_> = values.keys().collect();
                keys.sort();
                if keys
                    .iter()
                    .any(|key| key.is_empty() || !key.bytes().all(|b| (0x20..=0x7e).contains(&b)))
                {
                    return Err("content object keys must be printable ASCII".into());
                }
                for (i, key) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    // Authored keys/IDs are ASCII; strings use ordinary JSON escaping.
                    out.push_str(&serde_json::to_string(key).map_err(|e| e.to_string())?);
                    out.push(':');
                    write(&values[*key], out)?;
                }
                out.push('}');
            }
            _ => out.push_str(&serde_json::to_string(value).map_err(|e| e.to_string())?),
        }
        Ok(())
    }
    write(&value, &mut output)?;
    Ok(output.into_bytes())
}
pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn manifest_hash(manifest: &crate::WorldManifest) -> Result<String> {
    let mut value = serde_json::to_value(manifest).map_err(|e| e.to_string())?;
    value
        .as_object_mut()
        .ok_or("manifest object")?
        .remove("content_hash");
    Ok(sha256(&bytes(&value)?))
}
