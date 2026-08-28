//! Per-upload `process_overrides` merged with ProductVersion (brain ResolveProcessConfig).

use crate::store::ProductVersion;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParserEngineRule {
    #[serde(default)]
    pub file_types: Vec<String>,
    #[serde(default)]
    pub engine: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChunkingOverride {
    #[serde(default)]
    pub chunk_size: Option<usize>,
    #[serde(default)]
    pub chunk_overlap: Option<usize>,
    #[serde(default)]
    pub strategy: Option<String>,
    /// Whole-value replace when `chunking_config` is present (brain EnableParentChild).
    #[serde(default)]
    pub enable_parent_child: bool,
    #[serde(default)]
    pub parent_chunk_size: Option<usize>,
    #[serde(default)]
    pub child_chunk_size: Option<usize>,
    #[serde(default)]
    pub separators: Vec<String>,
    #[serde(default)]
    pub token_limit: Option<usize>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub parser_engine_rules: Vec<ParserEngineRule>,
    #[serde(default)]
    pub table_metadata_instructions: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExtractOverride {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct QuestionOverride {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub question_count: Option<usize>,
    #[serde(default)]
    pub custom_instructions: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AsrOverride {
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProcessOverrides {
    #[serde(default)]
    pub parser_engine_rules: Vec<ParserEngineRule>,
    #[serde(default)]
    pub chunking_config: Option<ChunkingOverride>,
    #[serde(default)]
    pub enable_multimodel: Option<bool>,
    #[serde(default)]
    pub graph_enabled: Option<bool>,
    #[serde(default)]
    pub extract_config: Option<ExtractOverride>,
    #[serde(default)]
    pub question_generation_config: Option<QuestionOverride>,
    #[serde(default)]
    pub asr_config: Option<AsrOverride>,
    #[serde(default)]
    pub parser_engine_overrides: std::collections::HashMap<String, String>,
}

impl ProcessOverrides {
    pub fn is_empty(&self) -> bool {
        self.parser_engine_rules.is_empty()
            && self.chunking_config.is_none()
            && self.enable_multimodel.is_none()
            && self.graph_enabled.is_none()
            && self.extract_config.is_none()
            && self.question_generation_config.is_none()
            && self.asr_config.is_none()
            && self.parser_engine_overrides.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveProcessConfig {
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub chunk_strategy: String,
    pub enable_parent_child: bool,
    pub parent_chunk_size: usize,
    pub child_chunk_size: usize,
    pub chunk_separators: Vec<String>,
    pub chunk_token_limit: usize,
    pub chunk_languages: Vec<String>,
    pub parser_engine_rules: Vec<ParserEngineRule>,
    pub table_metadata_instructions: String,
    pub enable_multimodel: bool,
    pub graph_enabled: bool,
    pub extract_enabled: bool,
    pub extract_custom_instructions: String,
    pub question_enabled: bool,
    pub question_count: usize,
    pub question_custom_instructions: String,
    pub asr_enabled: bool,
}

impl EffectiveProcessConfig {
    pub fn apply_to(&self, version: &mut ProductVersion) {
        version.chunk_size = self.chunk_size;
        version.chunk_overlap = self.chunk_overlap;
        version.chunk_strategy = self.chunk_strategy.clone();
        version.enable_parent_child = self.enable_parent_child;
        version.parent_chunk_size = self.parent_chunk_size;
        version.child_chunk_size = self.child_chunk_size;
        version.chunk_separators = self.chunk_separators.clone();
        version.chunk_token_limit = self.chunk_token_limit;
        version.chunk_languages = self.chunk_languages.clone();
        version.parser_engine_rules = self.parser_engine_rules.clone();
        version.table_metadata_instructions = self.table_metadata_instructions.clone();
        version.enable_multimodel = self.enable_multimodel;
        version.graph_enabled = self.graph_enabled;
        version.extract_enabled = self.extract_enabled;
        version.extract_custom_instructions = self.extract_custom_instructions.clone();
        version.question_enabled = self.question_enabled;
        version.question_count = self.question_count;
        version.question_custom_instructions = self.question_custom_instructions.clone();
        version.asr_enabled = self.asr_enabled;
    }
}

pub fn resolve_process_config(
    base: &ProductVersion,
    overrides: Option<&ProcessOverrides>,
) -> EffectiveProcessConfig {
    let mut eff = EffectiveProcessConfig {
        chunk_size: base.chunk_size,
        chunk_overlap: base.chunk_overlap,
        chunk_strategy: base.chunk_strategy.clone(),
        enable_parent_child: base.enable_parent_child,
        parent_chunk_size: base.parent_chunk_size,
        child_chunk_size: base.child_chunk_size,
        chunk_separators: base.chunk_separators.clone(),
        chunk_token_limit: base.chunk_token_limit,
        chunk_languages: base.chunk_languages.clone(),
        parser_engine_rules: base.parser_engine_rules.clone(),
        table_metadata_instructions: base.table_metadata_instructions.clone(),
        enable_multimodel: base.enable_multimodel,
        graph_enabled: base.graph_enabled,
        extract_enabled: base.extract_enabled,
        extract_custom_instructions: base.extract_custom_instructions.clone(),
        question_enabled: base.question_enabled,
        question_count: base.question_count(),
        question_custom_instructions: base.question_custom_instructions.clone(),
        asr_enabled: base.asr_enabled,
    };
    let Some(o) = overrides else {
        eff.graph_enabled = eff.graph_enabled && eff.extract_enabled;
        return eff;
    };
    if let Some(c) = &o.chunking_config {
        if let Some(n) = c.chunk_size.filter(|n| *n > 0) {
            eff.chunk_size = n;
        }
        if let Some(n) = c.chunk_overlap.filter(|n| *n > 0) {
            eff.chunk_overlap = n;
        }
        if let Some(s) = c.strategy.as_ref().filter(|s| !s.is_empty()) {
            eff.chunk_strategy = s.clone();
        }
        eff.enable_parent_child = c.enable_parent_child;
        if let Some(n) = c.parent_chunk_size.filter(|n| *n > 0) {
            eff.parent_chunk_size = n;
        }
        if let Some(n) = c.child_chunk_size.filter(|n| *n > 0) {
            eff.child_chunk_size = n;
        }
        if !c.separators.is_empty() {
            eff.chunk_separators = c.separators.clone();
        }
        if let Some(n) = c.token_limit.filter(|n| *n > 0) {
            eff.chunk_token_limit = n;
        }
        if !c.languages.is_empty() {
            eff.chunk_languages = c.languages.clone();
        }
        if !c.parser_engine_rules.is_empty() {
            eff.parser_engine_rules = c.parser_engine_rules.clone();
        }
        if !c.table_metadata_instructions.trim().is_empty() {
            eff.table_metadata_instructions = c.table_metadata_instructions.clone();
        }
    }
    if !o.parser_engine_rules.is_empty() {
        eff.parser_engine_rules = o.parser_engine_rules.clone();
    }
    if let Some(v) = o.enable_multimodel {
        eff.enable_multimodel = v;
    }
    if let Some(v) = o.graph_enabled {
        eff.graph_enabled = v;
    }
    if let Some(ex) = &o.extract_config {
        eff.extract_enabled = ex.enabled;
        if !ex.text.is_empty() {
            eff.extract_custom_instructions = ex.text.clone();
        }
    }
    if let Some(q) = &o.question_generation_config {
        if let Some(v) = q.enabled {
            eff.question_enabled = v;
        }
        if let Some(n) = q.question_count.filter(|n| *n > 0) {
            eff.question_count = n.min(10);
        }
        if let Some(s) = q.custom_instructions.as_ref().filter(|s| !s.is_empty()) {
            eff.question_custom_instructions = s.clone();
        }
    }
    if let Some(a) = &o.asr_config
        && let Some(v) = a.enabled
    {
        eff.asr_enabled = v;
    }
    eff.graph_enabled = eff.graph_enabled && eff.extract_enabled;
    eff
}

pub fn engine_for_file(rules: &[ParserEngineRule], file_type: &str) -> String {
    let ft = file_type.trim_start_matches('.').to_ascii_lowercase();
    for rule in rules {
        if rule
            .file_types
            .iter()
            .any(|t| t.trim_start_matches('.').eq_ignore_ascii_case(&ft))
        {
            return rule.engine.clone();
        }
    }
    String::new()
}

/// Product default: office → anydoc (tables), PDF → builtin (layout + scan OCR).
pub fn default_parser_engine_rules() -> Vec<ParserEngineRule> {
    vec![
        ParserEngineRule {
            file_types: vec![
                "docx".into(),
                "doc".into(),
                "docm".into(),
                "xlsx".into(),
                "xls".into(),
                "xlsm".into(),
                "pptx".into(),
                "ppt".into(),
                "pptm".into(),
            ],
            engine: "anydoc".into(),
        },
        ParserEngineRule {
            file_types: vec!["pdf".into()],
            engine: "builtin".into(),
        },
    ]
}

/// Empty rules mean the product default, not MarkItDown. Bare engine is only
/// used when no rule matches the extension (md/txt/csv stay empty → simple).
pub fn resolve_parser_engine(
    rules: &[ParserEngineRule],
    bare_engine: &str,
    file_type: &str,
) -> String {
    let default_rules;
    let use_rules: &[ParserEngineRule] = if rules.is_empty() {
        default_rules = default_parser_engine_rules();
        &default_rules
    } else {
        rules
    };
    let engine = engine_for_file(use_rules, file_type);
    if engine.is_empty() {
        bare_engine.to_string()
    } else {
        engine
    }
}

/// Knowledge `document:process` and bid convert share this entry.
pub fn parser_engine_for(
    chunking: &serde_json::Value,
    overrides: Option<&ProcessOverrides>,
    ext: &str,
) -> String {
    let mut rules: Vec<ParserEngineRule> = chunking
        .get("parser_engine_rules")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    if let Some(o) = overrides {
        if !o.parser_engine_rules.is_empty() {
            rules = o.parser_engine_rules.clone();
        } else if let Some(c) = &o.chunking_config
            && !c.parser_engine_rules.is_empty()
        {
            rules = c.parser_engine_rules.clone();
        }
    }
    let bare = chunking
        .get("parser_engine")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    resolve_parser_engine(&rules, bare, ext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ProductVersion;
    use uuid::Uuid;

    fn base() -> ProductVersion {
        let mut v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        v.chunk_size = 512;
        v.chunk_overlap = 80;
        v.enable_parent_child = true;
        v.graph_enabled = true;
        v.extract_enabled = true;
        v.enable_multimodel = false;
        v.parser_engine_rules = vec![ParserEngineRule {
            file_types: vec!["pdf".into()],
            engine: "builtin".into(),
        }];
        v
    }

    #[test]
    fn none_keeps_base_and_ands_graph() {
        let mut off = base();
        off.extract_enabled = false;
        let eff = resolve_process_config(&off, None);
        assert!(!eff.graph_enabled);
        assert_eq!(eff.chunk_size, 512);
    }

    #[test]
    fn parent_child_whole_value_and_zero_size_keeps_base() {
        let o = ProcessOverrides {
            chunking_config: Some(ChunkingOverride {
                chunk_size: Some(0),
                enable_parent_child: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        let eff = resolve_process_config(&base(), Some(&o));
        assert_eq!(eff.chunk_size, 512);
        assert!(!eff.enable_parent_child);
    }

    #[test]
    fn parser_engine_rules_replace_table() {
        let o = ProcessOverrides {
            parser_engine_rules: vec![ParserEngineRule {
                file_types: vec!["docx".into()],
                engine: "mineru".into(),
            }],
            ..Default::default()
        };
        let eff = resolve_process_config(&base(), Some(&o));
        assert_eq!(eff.parser_engine_rules.len(), 1);
        assert_eq!(eff.parser_engine_rules[0].file_types, ["docx"]);
        assert_eq!(engine_for_file(&eff.parser_engine_rules, "DOCX"), "mineru");
        assert!(engine_for_file(&eff.parser_engine_rules, "pdf").is_empty());
    }

    #[test]
    fn default_engine_table_office_anydoc_pdf_builtin() {
        let empty = serde_json::json!({});
        assert_eq!(parser_engine_for(&empty, None, "docx"), "anydoc");
        assert_eq!(parser_engine_for(&empty, None, "DOCX"), "anydoc");
        assert_eq!(parser_engine_for(&empty, None, "xlsx"), "anydoc");
        assert_eq!(parser_engine_for(&empty, None, "ppt"), "anydoc");
        assert_eq!(parser_engine_for(&empty, None, "pdf"), "builtin");
        assert_eq!(parser_engine_for(&empty, None, "txt"), "");
        let bare = serde_json::json!({"parser_engine": "builtin"});
        assert_eq!(parser_engine_for(&bare, None, "docx"), "anydoc");
        assert_eq!(parser_engine_for(&bare, None, "txt"), "builtin");
        let mineru = serde_json::json!({
            "parser_engine": "builtin",
            "parser_engine_rules": [{"file_types": ["pdf"], "engine": "mineru"}]
        });
        assert_eq!(parser_engine_for(&mineru, None, "pdf"), "mineru");
        assert_eq!(parser_engine_for(&mineru, None, "txt"), "builtin");
        let paddle = ProcessOverrides {
            parser_engine_rules: vec![ParserEngineRule {
                file_types: vec!["pdf".into()],
                engine: "paddle".into(),
            }],
            ..Default::default()
        };
        assert_eq!(parser_engine_for(&mineru, Some(&paddle), "pdf"), "paddle");
        let o = ProcessOverrides {
            parser_engine_rules: vec![ParserEngineRule {
                file_types: vec!["docx".into()],
                engine: "builtin".into(),
            }],
            ..Default::default()
        };
        assert_eq!(parser_engine_for(&mineru, Some(&o), "pdf"), "builtin");
    }

    #[test]
    fn graph_override_and_extract() {
        let kb = base();
        let off = ProcessOverrides {
            graph_enabled: Some(false),
            ..Default::default()
        };
        assert!(!resolve_process_config(&kb, Some(&off)).graph_enabled);
        let on_but_extract_off = ProcessOverrides {
            graph_enabled: Some(true),
            extract_config: Some(ExtractOverride {
                enabled: false,
                text: String::new(),
            }),
            ..Default::default()
        };
        assert!(!resolve_process_config(&kb, Some(&on_but_extract_off)).graph_enabled);
    }

    #[test]
    fn parent_child_sizes_and_separators_merge() {
        let o = ProcessOverrides {
            chunking_config: Some(ChunkingOverride {
                parent_chunk_size: Some(2000),
                child_chunk_size: Some(200),
                separators: vec!["\n\n".into()],
                token_limit: Some(128),
                languages: vec!["zh".into()],
                enable_parent_child: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let eff = resolve_process_config(&base(), Some(&o));
        assert_eq!(eff.parent_chunk_size, 2000);
        assert_eq!(eff.child_chunk_size, 200);
        assert_eq!(eff.chunk_separators, ["\n\n"]);
        assert_eq!(eff.chunk_token_limit, 128);
        assert_eq!(eff.chunk_languages, ["zh"]);
        assert!(eff.enable_parent_child);
        let keep = ProcessOverrides {
            chunking_config: Some(ChunkingOverride {
                parent_chunk_size: Some(0),
                enable_parent_child: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            resolve_process_config(&base(), Some(&keep)).parent_chunk_size,
            0
        );
    }

    #[test]
    fn question_override_clamps_and_keeps_instructions() {
        let mut kb = base();
        kb.question_count = 0;
        kb.question_custom_instructions = "audience: ops".into();
        let none = resolve_process_config(&kb, None);
        assert_eq!(none.question_count, 3);
        assert_eq!(none.question_custom_instructions, "audience: ops");
        let o = ProcessOverrides {
            question_generation_config: Some(QuestionOverride {
                enabled: Some(true),
                question_count: Some(99),
                custom_instructions: Some("exam".into()),
            }),
            ..Default::default()
        };
        let eff = resolve_process_config(&kb, Some(&o));
        assert_eq!(eff.question_count, 10);
        assert_eq!(eff.question_custom_instructions, "exam");
        assert!(eff.question_enabled);
    }

    #[test]
    fn table_metadata_instructions_merge() {
        let mut kb = base();
        kb.table_metadata_instructions = "units in Mbps".into();
        let none = resolve_process_config(&kb, None);
        assert_eq!(none.table_metadata_instructions, "units in Mbps");
        let o = ProcessOverrides {
            chunking_config: Some(ChunkingOverride {
                table_metadata_instructions: "ports only".into(),
                enable_parent_child: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let eff = resolve_process_config(&kb, Some(&o));
        assert_eq!(eff.table_metadata_instructions, "ports only");
        let empty = ProcessOverrides {
            chunking_config: Some(ChunkingOverride {
                enable_parent_child: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            resolve_process_config(&kb, Some(&empty)).table_metadata_instructions,
            "units in Mbps"
        );
    }
}
