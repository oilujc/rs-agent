use crate::error::Result;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct AgentPrompt {
    sections: BTreeMap<String, String>,
    raw: Option<String>,
}

impl AgentPrompt {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    pub fn from_str(content: &str) -> Result<Self> {
        let mut sections = BTreeMap::new();
        let mut current_header = None;
        let mut current_content = String::new();

        for line in content.lines() {
            if let Some(stripped) = line.strip_prefix("## ") {
                if let Some(header) = current_header.take() {
                    sections.insert(header, current_content.trim().to_string());
                    current_content = String::new();
                }
                current_header = Some(stripped.trim().to_string());
            } else {
                current_content.push_str(line);
                current_content.push('\n');
            }
        }

        if let Some(header) = current_header {
            sections.insert(header, current_content.trim().to_string());
        }

        let raw = if sections.is_empty() {
            Some(content.trim().to_string())
        } else {
            None
        };

        Ok(Self { sections, raw })
    }

    pub fn to_system_prompt(&self) -> String {
        if let Some(raw) = &self.raw {
            return raw.clone();
        }

        let mut prompt = String::new();
        for (header, content) in &self.sections {
            prompt.push_str(&format!("## {}\n{}\n\n", header, content));
        }
        prompt.trim().to_string()
    }

    pub fn get_section(&self, name: &str) -> Option<&str> {
        self.sections.get(name).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_structured_prompt() {
        let content = r#"## Role
You are a helpful assistant.

## Context
You specialize in coding.
"#;
        let prompt = AgentPrompt::from_str(content).unwrap();
        assert_eq!(
            prompt.get_section("Role"),
            Some("You are a helpful assistant.")
        );
        assert_eq!(
            prompt.get_section("Context"),
            Some("You specialize in coding.")
        );
    }

    #[test]
    fn test_fallback_raw_prompt() {
        let content = "Just a plain system prompt without headers.";
        let prompt = AgentPrompt::from_str(content).unwrap();
        assert_eq!(
            prompt.to_system_prompt(),
            "Just a plain system prompt without headers."
        );
    }
}
