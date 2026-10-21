//! Allocation-bounded MessagePack preflight. Maps must have unique string keys.
use crate::{Result, invalid, limits::*};
use std::collections::BTreeSet;

pub fn scan(bytes: &[u8], byte_limit: usize) -> Result<()> {
    if bytes.is_empty() || bytes.len() > byte_limit {
        return Err(invalid("empty or oversized frame"));
    }
    let mut scanner = Scanner {
        bytes,
        offset: 0,
        nodes: 0,
    };
    scanner.value(0)?;
    if scanner.offset != bytes.len() {
        return Err(invalid("trailing MessagePack data"));
    }
    Ok(())
}

struct Scanner<'a> {
    bytes: &'a [u8],
    offset: usize,
    nodes: usize,
}

impl<'a> Scanner<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| invalid("invalid length"))?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| invalid("truncated frame"))?;
        self.offset = end;
        Ok(result)
    }

    fn unsigned(&mut self, count: usize) -> Result<u64> {
        let mut result = 0_u64;
        for byte in self.take(count)? {
            result = (result << 8) | u64::from(*byte);
        }
        Ok(result)
    }

    fn bump(&mut self, depth: usize) -> Result<()> {
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES {
            return Err(invalid("MessagePack complexity limit"));
        }
        Ok(())
    }

    fn string(&mut self, marker: u8) -> Result<&'a str> {
        let count = match marker {
            0xa0..=0xbf => u64::from(marker & 0x1f),
            0xd9 => self.unsigned(1)?,
            0xda => self.unsigned(2)?,
            0xdb => self.unsigned(4)?,
            _ => return Err(invalid("map keys must be strings")),
        };
        if count > MAX_STRING_BYTES as u64 {
            return Err(invalid("string length limit"));
        }
        std::str::from_utf8(self.take(count as usize)?).map_err(|_| invalid("invalid UTF-8"))
    }

    fn array(&mut self, count: u64, depth: usize) -> Result<()> {
        if count > MAX_ARRAY_ITEMS as u64 {
            return Err(invalid("array length limit"));
        }
        for _ in 0..count {
            self.value(depth + 1)?;
        }
        Ok(())
    }

    fn map(&mut self, count: u64, depth: usize) -> Result<()> {
        if count > MAX_MAP_FIELDS as u64 {
            return Err(invalid("map length limit"));
        }
        let mut keys = BTreeSet::new();
        for _ in 0..count {
            self.bump(depth + 1)?;
            let marker = self.unsigned(1)? as u8;
            let key = self.string(marker)?;
            if matches!(key, "__proto__" | "constructor" | "prototype") || !keys.insert(key) {
                return Err(invalid("duplicate or reserved map key"));
            }
            self.value(depth + 1)?;
        }
        Ok(())
    }

    fn value(&mut self, depth: usize) -> Result<()> {
        self.bump(depth)?;
        let marker = self.unsigned(1)? as u8;
        match marker {
            0x00..=0x7f | 0xe0..=0xff | 0xc0 | 0xc2 | 0xc3 => Ok(()),
            0x80..=0x8f => self.map(u64::from(marker & 0x0f), depth),
            0x90..=0x9f => self.array(u64::from(marker & 0x0f), depth),
            0xa0..=0xbf | 0xd9..=0xdb => {
                self.string(marker)?;
                Ok(())
            }
            0xcc..=0xcf => {
                let size = 1 << (marker - 0xcc);
                if self.unsigned(size)? > MAX_SAFE_INTEGER {
                    return Err(invalid("unsafe integer"));
                }
                Ok(())
            }
            0xd0..=0xd3 => {
                let size = 1 << (marker - 0xd0);
                let raw = self.unsigned(size)?;
                let value = ((raw << (64 - size * 8)) as i64) >> (64 - size * 8);
                if value.unsigned_abs() > MAX_SAFE_INTEGER {
                    return Err(invalid("unsafe integer"));
                }
                Ok(())
            }
            0xca | 0xcb => {
                let value = if marker == 0xca {
                    f64::from(f32::from_bits(self.unsigned(4)? as u32))
                } else {
                    f64::from_bits(self.unsigned(8)?)
                };
                if !value.is_finite() || value.abs() > f64::from(f32::MAX) {
                    return Err(invalid("non-finite or unrepresentable float"));
                }
                Ok(())
            }
            0xdc | 0xdd => {
                let count = self.unsigned(if marker == 0xdc { 2 } else { 4 })?;
                self.array(count, depth)
            }
            0xde | 0xdf => {
                let count = self.unsigned(if marker == 0xde { 2 } else { 4 })?;
                self.map(count, depth)
            }
            _ => Err(invalid(
                "unsupported MessagePack marker (binary/extensions forbidden)",
            )),
        }
    }
}
