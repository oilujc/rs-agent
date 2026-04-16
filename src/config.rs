use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct ProviderConfig {
    #[serde(default = "default_provider_name")]
    pub name: String,

    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default = "default_url")]
    pub url: String,

    #[serde(default)]
    pub temperature: Option<f32>,

    #[serde(default)]
    pub max_tokens: Option<u32>,

    #[serde(default)]
    pub summary_model: Option<String>,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default)]
    pub think: bool,
}

fn default_provider_name() -> String {
    "ollama".to_string()
}

fn default_model() -> String {
    "llama3.2".to_string()
}

fn default_url() -> String {
    "http://localhost:11434".to_string()
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            name: default_provider_name(),
            model: default_model(),
            url: default_url(),
            temperature: None,
            max_tokens: None,
            summary_model: None,
            api_key: None,
            think: false,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Config {
    #[serde(default)]
    pub provider: ProviderConfig,

    #[serde(default)]
    pub db_path: Option<String>,

    #[serde(default)]
    pub workdir: Option<PathBuf>,

    #[serde(default = "default_context_messages")]
    pub context_messages: u32,

    #[serde(default = "default_summarize")]
    pub summarize: bool,
}

fn default_context_messages() -> u32 {
    3
}

fn default_summarize() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: ProviderConfig::default(),
            db_path: None,
            workdir: None,
            context_messages: default_context_messages(),
            summarize: default_summarize(),
        }
    }
}

impl Config {
    pub fn from_file(path: &Path) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn merge_cli_overrides(
        mut self,
        provider_name: Option<String>,
        model: Option<String>,
        url: Option<String>,
        temperature: Option<f32>,
        api_key: Option<String>,
        db_path: Option<String>,
        workdir: Option<PathBuf>,
        context_messages: Option<u32>,
        no_summarize: bool,
        think: bool,
        max_tokens: Option<u32>,
    ) -> Self {
        if let Some(n) = provider_name {
            self.provider.name = n;
        }
        if let Some(m) = model {
            self.provider.model = m;
        }
        if let Some(u) = url {
            self.provider.url = u;
        }
        if let Some(t) = temperature {
            self.provider.temperature = Some(t);
        }
        if let Some(k) = api_key {
            self.provider.api_key = Some(k);
        }
        if let Some(d) = db_path {
            self.db_path = Some(d);
        }
        if let Some(w) = workdir {
            self.workdir = Some(w);
        }
        if let Some(cm) = context_messages {
            self.context_messages = cm;
        }
        if no_summarize {
            self.summarize = false;
        }
        if think {
            self.provider.think = true;
        }
        if let Some(mt) = max_tokens {
            self.provider.max_tokens = Some(mt);
        }
        self
    }

    pub fn resolved_workdir(&self) -> Option<PathBuf> {
        self.workdir.as_ref().map(|w| {
            if w.is_absolute() {
                w.clone()
            } else {
                std::env::current_dir().unwrap_or_default().join(w)
            }
        })
    }
}
