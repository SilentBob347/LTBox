//! KonaBess URI and machine-generated DTS-like frequency-table parsing.

use std::io::{Cursor, Read};

use ltbox_core::Result;
use serde::Deserialize;

use super::error;

const URI_PREFIX: &str = "konabess://";
const MAX_EXPORT_JSON_SIZE: usize = 4 * 1024 * 1024;

/// One ordered u32-cell property in a GPU group or power level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuProperty {
    pub name: String,
    pub cells: Vec<u32>,
}

/// One `qcom,gpu-pwrlevel@N` node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuLevel {
    pub id: u32,
    pub properties: Vec<GpuProperty>,
}

/// One `qcom,gpu-pwrlevels-N` node, including its ordered header properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuGroup {
    pub id: u32,
    pub header_properties: Vec<GpuProperty>,
    pub levels: Vec<GpuLevel>,
}

/// Complete ordered GPU power-level sibling set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuTable {
    pub groups: Vec<GpuGroup>,
}

/// Validated data carried by a KonaBess export URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KonaBessExport {
    pub chip: String,
    pub description: String,
    pub table: GpuTable,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportJson {
    chip: String,
    desc: String,
    freq: String,
    #[serde(default)]
    volt: Option<serde_json::Value>,
}

/// Parse a complete `konabess://` export string.
pub fn parse_export(input: &str) -> Result<KonaBessExport> {
    let input = input.trim();
    let payload = input
        .strip_prefix(URI_PREFIX)
        .ok_or_else(|| error("export must start with konabess://"))?;
    if payload.is_empty() {
        return Err(error("export payload is empty"));
    }

    let compressed = decode_base64(payload)?;
    let json_bytes = decode_gzip(&compressed)?;
    let raw: ExportJson = serde_json::from_slice(&json_bytes)
        .map_err(|e| error(format!("invalid export JSON: {e}")))?;
    if raw.volt.is_some() {
        return Err(error("voltage tables are not supported"));
    }
    if raw.chip.trim().is_empty() {
        return Err(error("export chip is empty"));
    }

    Ok(KonaBessExport {
        chip: raw.chip,
        description: raw.desc,
        table: parse_frequency_table(&raw.freq)?,
    })
}

fn decode_base64(input: &str) -> Result<Vec<u8>> {
    if !input.len().is_multiple_of(4) {
        return Err(error("base64 payload length is not a multiple of four"));
    }
    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    for (block_index, block) in input.as_bytes().chunks_exact(4).enumerate() {
        let is_last = block_index + 1 == input.len() / 4;
        let padding = match (block[2], block[3]) {
            (b'=', b'=') => 2,
            (_, b'=') => 1,
            (b'=', _) => return Err(error("invalid base64 padding")),
            _ => 0,
        };
        if padding != 0 && !is_last {
            return Err(error("base64 padding is only valid in the final block"));
        }

        let a = base64_value(block[0])?;
        let b = base64_value(block[1])?;
        let c = if padding == 2 {
            0
        } else {
            base64_value(block[2])?
        };
        let d = if padding == 0 {
            base64_value(block[3])?
        } else {
            0
        };
        if (padding == 2 && b & 0x0f != 0) || (padding == 1 && c & 0x03 != 0) {
            return Err(error("base64 payload has non-zero padding bits"));
        }

        output.push((a << 2) | (b >> 4));
        if padding < 2 {
            output.push((b << 4) | (c >> 2));
        }
        if padding == 0 {
            output.push((c << 6) | d);
        }
    }
    Ok(output)
}

fn base64_value(byte: u8) -> Result<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(error(format!("invalid base64 character 0x{byte:02x}"))),
    }
}

fn decode_gzip(input: &[u8]) -> Result<Vec<u8>> {
    if input.len() < 18 || input[..3] != [0x1f, 0x8b, 8] {
        return Err(error("payload is not a gzip stream"));
    }
    let flags = input[3];
    if flags & 0xe0 != 0 {
        return Err(error("gzip header uses reserved flags"));
    }
    let footer_start = input.len() - 8;
    let mut position = 10usize;
    if flags & 0x04 != 0 {
        let length_bytes = input
            .get(position..position + 2)
            .ok_or_else(|| error("truncated gzip extra header"))?;
        let length = usize::from(u16::from_le_bytes([length_bytes[0], length_bytes[1]]));
        position = position
            .checked_add(2 + length)
            .ok_or_else(|| error("gzip header length overflow"))?;
    }
    if flags & 0x08 != 0 {
        position = skip_gzip_c_string(input, position, footer_start, "file name")?;
    }
    if flags & 0x10 != 0 {
        position = skip_gzip_c_string(input, position, footer_start, "comment")?;
    }
    if flags & 0x02 != 0 {
        position = position
            .checked_add(2)
            .ok_or_else(|| error("gzip header length overflow"))?;
    }
    if position > footer_start {
        return Err(error("truncated gzip header"));
    }

    let compressed = &input[position..footer_start];
    let crc = &input[footer_start..footer_start + 4];
    let size = &input[footer_start + 4..];
    let expected_size = u32::from_le_bytes([size[0], size[1], size[2], size[3]]) as usize;
    if expected_size > MAX_EXPORT_JSON_SIZE {
        return Err(error(format!(
            "decompressed export exceeds {MAX_EXPORT_JSON_SIZE} bytes"
        )));
    }
    let compressed_size =
        u32::try_from(compressed.len()).map_err(|_| error("compressed export is too large"))?;

    // `zip` is already a patch-crate dependency. A ZIP local entry carries the
    // same raw DEFLATE stream, CRC32, and uncompressed size as gzip, so a tiny
    // synthetic local header lets its checked in-process decoder handle gzip
    // without adding another compression dependency or invoking a tool.
    let mut local_entry = Vec::with_capacity(31 + compressed.len());
    local_entry.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    local_entry.extend_from_slice(&20u16.to_le_bytes());
    local_entry.extend_from_slice(&0u16.to_le_bytes());
    local_entry.extend_from_slice(&8u16.to_le_bytes());
    local_entry.extend_from_slice(&0u16.to_le_bytes());
    local_entry.extend_from_slice(&0u16.to_le_bytes());
    local_entry.extend_from_slice(crc);
    local_entry.extend_from_slice(&compressed_size.to_le_bytes());
    local_entry.extend_from_slice(&(expected_size as u32).to_le_bytes());
    local_entry.extend_from_slice(&1u16.to_le_bytes());
    local_entry.extend_from_slice(&0u16.to_le_bytes());
    local_entry.push(b'x');
    local_entry.extend_from_slice(compressed);

    let mut cursor = Cursor::new(local_entry);
    let mut entry = zip::read::read_zipfile_from_stream(&mut cursor)
        .map_err(|e| error(format!("cannot decompress gzip payload: {e}")))?
        .ok_or_else(|| error("gzip payload did not contain a DEFLATE stream"))?;
    let mut output = Vec::with_capacity(expected_size);
    entry
        .read_to_end(&mut output)
        .map_err(|e| error(format!("cannot decompress gzip payload: {e}")))?;
    if output.len() != expected_size {
        return Err(error(format!(
            "gzip size mismatch: header says {expected_size}, decoded {}",
            output.len()
        )));
    }
    Ok(output)
}

fn skip_gzip_c_string(input: &[u8], start: usize, limit: usize, field: &str) -> Result<usize> {
    let relative = input
        .get(start..limit)
        .and_then(|bytes| bytes.iter().position(|&byte| byte == 0))
        .ok_or_else(|| error(format!("unterminated gzip {field}")))?;
    Ok(start + relative + 1)
}

fn parse_frequency_table(input: &str) -> Result<GpuTable> {
    let mut groups = Vec::<GpuGroup>::new();
    let mut current_group: Option<GpuGroup> = None;
    let mut current_level: Option<GpuLevel> = None;

    for (line_index, raw_line) in input.lines().enumerate() {
        let line_number = line_index + 1;
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(id) = parse_node_header(line, "qcom,gpu-pwrlevels-", line_number)? {
            if current_group.is_some() || current_level.is_some() {
                return Err(line_error(line_number, "nested GPU group"));
            }
            current_group = Some(GpuGroup {
                id,
                header_properties: Vec::new(),
                levels: Vec::new(),
            });
            continue;
        }
        if let Some(id) = parse_node_header(line, "qcom,gpu-pwrlevel@", line_number)? {
            if current_group.is_none() || current_level.is_some() {
                return Err(line_error(line_number, "power level outside a GPU group"));
            }
            current_level = Some(GpuLevel {
                id,
                properties: Vec::new(),
            });
            continue;
        }
        if line == "};" {
            if let Some(level) = current_level.take() {
                validate_level(&level, line_number)?;
                current_group
                    .as_mut()
                    .expect("level state requires a group")
                    .levels
                    .push(level);
            } else if let Some(group) = current_group.take() {
                validate_group(&group, line_number)?;
                groups.push(group);
            } else {
                return Err(line_error(line_number, "unmatched node terminator"));
            }
            continue;
        }

        let property = parse_property(line, line_number)?;
        if let Some(level) = current_level.as_mut() {
            if property.cells.len() != 1 {
                return Err(line_error(
                    line_number,
                    "power-level properties must contain exactly one u32 cell",
                ));
            }
            level.properties.push(property);
        } else if let Some(group) = current_group.as_mut() {
            if property.cells.len() != 1 && property.name != "qcom,sku-codes" {
                return Err(line_error(
                    line_number,
                    "only qcom,sku-codes may contain multiple cells",
                ));
            }
            group.header_properties.push(property);
        } else {
            return Err(line_error(line_number, "property outside a GPU group"));
        }
    }

    if current_level.is_some() || current_group.is_some() {
        return Err(error("frequency table ends inside a node"));
    }
    if groups.is_empty() {
        return Err(error("frequency table has no GPU groups"));
    }
    ensure_unique_ids(groups.iter().map(|group| group.id), "GPU group")?;
    Ok(GpuTable { groups })
}

fn parse_node_header(line: &str, prefix: &str, line_number: usize) -> Result<Option<u32>> {
    let Some(rest) = line.strip_prefix(prefix) else {
        return Ok(None);
    };
    let id_text = rest
        .strip_suffix(" {")
        .ok_or_else(|| line_error(line_number, "malformed node header"))?;
    if id_text.is_empty() || !id_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(line_error(line_number, "node id must be decimal"));
    }
    let id = id_text
        .parse()
        .map_err(|_| line_error(line_number, "node id does not fit u32"))?;
    Ok(Some(id))
}

fn parse_property(line: &str, line_number: usize) -> Result<GpuProperty> {
    let (name, value) = line
        .strip_suffix(';')
        .and_then(|line| line.split_once(" = "))
        .ok_or_else(|| line_error(line_number, "expected `name = <cells>;`"))?;
    if name.is_empty()
        || !name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'#' | b',' | b'-' | b'_' | b'.')
        })
    {
        return Err(line_error(line_number, "invalid property name"));
    }
    let cells_text = value
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .ok_or_else(|| line_error(line_number, "property value must be u32 cells"))?;
    let cells = cells_text
        .split_ascii_whitespace()
        .map(|cell| parse_cell(cell, line_number))
        .collect::<Result<Vec<_>>>()?;
    if cells.is_empty() {
        return Err(line_error(line_number, "property has no cells"));
    }
    Ok(GpuProperty {
        name: name.to_string(),
        cells,
    })
}

fn parse_cell(cell: &str, line_number: usize) -> Result<u32> {
    let parsed = if let Some(hex) = cell.strip_prefix("0x").or_else(|| cell.strip_prefix("0X")) {
        if hex.is_empty() {
            return Err(line_error(line_number, "empty hexadecimal cell"));
        }
        u32::from_str_radix(hex, 16)
    } else {
        cell.parse()
    };
    parsed.map_err(|_| line_error(line_number, format!("invalid u32 cell `{cell}`")))
}

fn validate_level(level: &GpuLevel, line_number: usize) -> Result<()> {
    if level.properties.is_empty() {
        return Err(line_error(line_number, "power level has no properties"));
    }
    ensure_unique_names(&level.properties, "power-level property")?;
    let reg = level
        .properties
        .iter()
        .find(|property| property.name == "reg")
        .ok_or_else(|| line_error(line_number, "power level has no reg property"))?;
    if reg.cells[0] != level.id {
        return Err(line_error(
            line_number,
            format!(
                "power-level node id {} does not match reg {}",
                level.id, reg.cells[0]
            ),
        ));
    }
    Ok(())
}

fn validate_group(group: &GpuGroup, line_number: usize) -> Result<()> {
    if group.levels.is_empty() {
        return Err(line_error(line_number, "GPU group has no power levels"));
    }
    ensure_unique_names(&group.header_properties, "group header property")?;
    let selector_count = group
        .header_properties
        .iter()
        .filter(|property| matches!(property.name.as_str(), "qcom,speed-bin" | "qcom,sku-codes"))
        .count();
    if selector_count == 0 {
        return Err(line_error(
            line_number,
            "GPU group must have a speed-bin or sku-codes selector",
        ));
    }
    ensure_unique_ids(group.levels.iter().map(|level| level.id), "power level")
}

fn ensure_unique_names(properties: &[GpuProperty], kind: &str) -> Result<()> {
    for (index, property) in properties.iter().enumerate() {
        if properties[..index]
            .iter()
            .any(|prior| prior.name == property.name)
        {
            return Err(error(format!("duplicate {kind} `{}`", property.name)));
        }
    }
    Ok(())
}

fn ensure_unique_ids(ids: impl IntoIterator<Item = u32>, kind: &str) -> Result<()> {
    let mut seen = Vec::new();
    for id in ids {
        if seen.contains(&id) {
            return Err(error(format!("duplicate {kind} id {id}")));
        }
        seen.push(id);
    }
    Ok(())
}

fn line_error(line: usize, message: impl Into<String>) -> ltbox_core::LtboxError {
    error(format!("frequency table line {line}: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_EXPORT: &str = "konabess://H4sIAAAAAAAACmWOywrCMBBFf6WM2wTiYxUf+CFu2mSsAzGmmUbF0n+XpEUUF7O551zmDmAuFEADJw8CLLIBDclTDwLOETvQ0JnbVbQhyfCIDu/oWKpqOPmSc0C0siFf7audejZ42M6EPPVUu09rEpaZL5heKA06x1OqSlpbG5H5GxT9b8Cx/I/YFulHyZvn6mq9yWgsB+MbZCgNFesAAAA=";

    #[test]
    fn parses_typed_export() {
        let export = parse_export(VALID_EXPORT).unwrap();
        assert_eq!(export.chip, "sun");
        assert_eq!(export.description, "unit");
        assert_eq!(export.table.groups.len(), 1);
        assert_eq!(export.table.groups[0].id, 0);
        assert_eq!(export.table.groups[0].levels[0].id, 0);
        assert_eq!(
            export.table.groups[0].levels[0].properties[1].cells,
            [0x1234]
        );
    }

    #[test]
    fn rejects_malformed_export() {
        let error = parse_export("konabess://not-base64").unwrap_err();
        assert!(error.to_string().contains("base64"));
    }

    #[test]
    fn rejects_narrow_format_violations() {
        let error = parse_frequency_table(
            "qcom,gpu-pwrlevels-0 {\nqcom,speed-bin = <0>;\nqcom,gpu-pwrlevel@1 {\nreg = <0>;\n};\n};",
        )
        .unwrap_err();
        assert!(error.to_string().contains("does not match reg"));
    }
}
