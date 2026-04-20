//! Partition catalog — parses Qualcomm EDL `rawprogram*.xml` into partition
//! records (label, filename, LUN, sectors) used during firmware flashing.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{LtboxError, Result};

#[derive(Debug, Clone)]
pub struct PartitionRecord {
    pub label: String,
    pub filename: String,
    pub lun: Option<String>,
    pub start_sector: Option<String>,
    pub num_sectors: Option<String>,
    pub size_in_kb: Option<String>,
    pub sector_size_bytes: Option<String>,
    pub source_xml: String,
}

impl PartitionRecord {
    /// "_a", "_b", or None.
    pub fn slot_suffix(&self) -> Option<&str> {
        if self.label.ends_with("_a") {
            Some("_a")
        } else if self.label.ends_with("_b") {
            Some("_b")
        } else {
            None
        }
    }

    pub fn is_ab(&self) -> bool {
        self.slot_suffix().is_some()
    }

    /// Label with the slot suffix stripped.
    pub fn base_label(&self) -> &str {
        if let Some(suffix) = self.slot_suffix() {
            &self.label[..self.label.len() - suffix.len()]
        } else {
            &self.label
        }
    }
}

/// A/B partitions sharing a base label.
#[derive(Debug, Clone, Default)]
pub struct PartitionGroup {
    pub base_label: String,
    pub records: Vec<PartitionRecord>,
}

impl PartitionGroup {
    pub fn is_ab(&self) -> bool {
        self.records.iter().any(|r| r.is_ab())
    }
}

/// Parsed partitions from one or more XML files.
pub struct XmlCatalog {
    records: Vec<PartitionRecord>,
}

impl XmlCatalog {
    pub fn from_paths(xml_paths: &[&Path]) -> Result<Self> {
        let mut records = Vec::new();
        for path in xml_paths {
            let mut parsed = parse_rawprogram_xml(path)?;
            records.append(&mut parsed);
        }
        Ok(Self { records })
    }

    /// Case-insensitive exact-label lookup.
    pub fn find(&self, label: &str) -> Option<&PartitionRecord> {
        let lower = label.to_lowercase();
        self.records
            .iter()
            .find(|r| r.label.to_lowercase() == lower)
    }

    /// Lookup with fallbacks (e.g. "boot" → "boot_a" → "boot_b").
    pub fn require(&self, label: &str, fallbacks: &[&str]) -> Result<&PartitionRecord> {
        if let Some(r) = self.find(label) {
            return Ok(r);
        }
        for fb in fallbacks {
            if let Some(r) = self.find(fb) {
                return Ok(r);
            }
        }
        Err(LtboxError::FileNotFound(format!(
            "Partition '{label}' not found in XML catalog"
        )))
    }

    pub fn group_by_base(&self) -> HashMap<String, PartitionGroup> {
        let mut groups: HashMap<String, PartitionGroup> = HashMap::new();
        for record in &self.records {
            let base = record.base_label().to_string();
            let group = groups
                .entry(base.clone())
                .or_insert_with(|| PartitionGroup {
                    base_label: base,
                    records: Vec::new(),
                });
            group.records.push(record.clone());
        }
        groups
    }

    pub fn records(&self) -> &[PartitionRecord] {
        &self.records
    }
}

fn parse_rawprogram_xml(path: &Path) -> Result<Vec<PartitionRecord>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| LtboxError::Config(format!("Cannot read XML {}: {e}", path.display())))?;

    let doc = roxmltree::Document::parse(&content)
        .map_err(|e| LtboxError::Config(format!("XML parse error in {}: {e}", path.display())))?;

    let source = path.display().to_string();
    let mut records = Vec::new();

    for node in doc.descendants() {
        if node.tag_name().name().eq_ignore_ascii_case("program") {
            let label = node.attribute("label").unwrap_or("").to_string();
            if label.is_empty() {
                continue;
            }
            records.push(PartitionRecord {
                label,
                filename: node.attribute("filename").unwrap_or("").to_string(),
                lun: node
                    .attribute("physical_partition_number")
                    .map(|s| s.to_string()),
                start_sector: node.attribute("start_sector").map(|s| s.to_string()),
                num_sectors: node
                    .attribute("num_partition_sectors")
                    .map(|s| s.to_string()),
                size_in_kb: node.attribute("size_in_KB").map(|s| s.to_string()),
                sector_size_bytes: node
                    .attribute("SECTOR_SIZE_IN_BYTES")
                    .map(|s| s.to_string()),
                source_xml: source.clone(),
            });
        }
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_partition_record() {
        let r = PartitionRecord {
            label: "boot_a".to_string(),
            filename: "boot.img".to_string(),
            lun: Some("4".to_string()),
            start_sector: Some("1024".to_string()),
            num_sectors: Some("2048".to_string()),
            size_in_kb: Some("4096".to_string()),
            sector_size_bytes: Some("4096".to_string()),
            source_xml: "test.xml".to_string(),
        };
        assert_eq!(r.slot_suffix(), Some("_a"));
        assert!(r.is_ab());
        assert_eq!(r.base_label(), "boot");
    }

    #[test]
    fn parse_xml_string() {
        let xml = r#"<?xml version="1.0" ?>
<data>
  <program label="boot_a" filename="boot.img" physical_partition_number="4"
           start_sector="1024" num_partition_sectors="2048"
           size_in_KB="4096" SECTOR_SIZE_IN_BYTES="4096" />
  <program label="vbmeta_a" filename="vbmeta.img" physical_partition_number="4"
           start_sector="8192" num_partition_sectors="16" />
</data>"#;
        let dir = tempfile::tempdir().unwrap();
        let xml_path = dir.path().join("rawprogram0.xml");
        std::fs::write(&xml_path, xml).unwrap();

        let records = parse_rawprogram_xml(&xml_path).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].label, "boot_a");
        assert_eq!(records[1].label, "vbmeta_a");
    }

    #[test]
    fn malformed_xml_returns_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let xml_path = dir.path().join("rawprogram_bad.xml");
        std::fs::write(&xml_path, "<data><program label=boot_a</data>").unwrap();
        let err = parse_rawprogram_xml(&xml_path).unwrap_err();
        assert!(matches!(err, LtboxError::Config(_)));
    }

    #[test]
    fn missing_file_returns_config_error() {
        let err = parse_rawprogram_xml(std::path::Path::new("/does/not/exist.xml")).unwrap_err();
        assert!(matches!(err, LtboxError::Config(_)));
    }

    #[test]
    fn entries_without_label_are_skipped() {
        // Real rawprogram XMLs include unlabeled GPT-placeholder entries.
        let xml = r#"<?xml version="1.0" ?>
<data>
  <program filename="" physical_partition_number="0"
           start_sector="0" num_partition_sectors="34" />
  <program label="boot_a" filename="boot.img" physical_partition_number="4"
           start_sector="1024" num_partition_sectors="2048" />
</data>"#;
        let dir = tempfile::tempdir().unwrap();
        let xml_path = dir.path().join("rawprogram0.xml");
        std::fs::write(&xml_path, xml).unwrap();
        let records = parse_rawprogram_xml(&xml_path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].label, "boot_a");
    }

    #[test]
    fn catalog_require_falls_back_to_slot_variant() {
        let r = PartitionRecord {
            label: "boot_a".to_string(),
            filename: "boot.img".to_string(),
            lun: None,
            start_sector: None,
            num_sectors: None,
            size_in_kb: None,
            sector_size_bytes: None,
            source_xml: String::new(),
        };
        let cat = XmlCatalog { records: vec![r] };
        assert!(cat.require("boot", &["boot_a", "boot_b"]).is_ok());
        assert!(cat.require("userdata", &[]).is_err());
    }
}
