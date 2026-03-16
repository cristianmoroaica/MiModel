use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub claude: ClaudeConfig,
    #[serde(default)]
    pub viewer: ViewerConfig,
    #[serde(default)]
    pub defaults: DefaultsConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeConfig {
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ViewerConfig {
    #[serde(default = "default_viewer")]
    pub command: String,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct DefaultsConfig {
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_build_timeout")]
    pub build_timeout: u64,
}

fn default_viewer() -> String { "f3d".to_string() }
fn default_output_dir() -> String { ".".to_string() }
fn default_max_retries() -> u32 { 3 }
fn default_build_timeout() -> u64 { 60 }

impl Default for Config {
    fn default() -> Self {
        Self { claude: ClaudeConfig::default(), viewer: ViewerConfig::default(), defaults: DefaultsConfig::default() }
    }
}

impl Default for ClaudeConfig {
    fn default() -> Self { Self { model: None } }
}

impl Default for ViewerConfig {
    fn default() -> Self { Self { command: default_viewer() } }
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self { output_dir: default_output_dir(), max_retries: default_max_retries(), build_timeout: default_build_timeout() }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if let Ok(config) = toml::from_str::<Config>(&contents) {
                    return config;
                }
            }
        }
        Config::default()
    }

    /// Resolve the Python interpreter to use for ai3d-cad.
    /// Priority: MIMODEL_PYTHON env var > .venv-cadquery/bin/python > python
    pub fn python_path(&self) -> String {
        if let Ok(p) = std::env::var("MIMODEL_PYTHON") {
            return p;
        }
        // Auto-detect project venv (relative to current dir or binary dir)
        for base in [std::env::current_dir().ok(), std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()))] {
            if let Some(base) = base {
                // Walk up to find .venv-cadquery
                let mut dir = base.as_path();
                loop {
                    let venv_python = dir.join(".venv-cadquery/bin/python");
                    if venv_python.exists() {
                        return venv_python.to_string_lossy().to_string();
                    }
                    match dir.parent() {
                        Some(parent) => dir = parent,
                        None => break,
                    }
                }
            }
        }
        "python".to_string()
    }

    fn config_path() -> PathBuf {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("mimodel").join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.claude.model, None);
        assert_eq!(config.viewer.command, "f3d");
        assert_eq!(config.defaults.max_retries, 3);
        assert_eq!(config.defaults.build_timeout, 60);
    }

    #[test]
    fn test_parse_toml() {
        let toml_str = r#"
[claude]
model = "sonnet"

[defaults]
build_timeout = 120
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.claude.model, Some("sonnet".to_string()));
        assert_eq!(config.defaults.build_timeout, 120);
        assert_eq!(config.defaults.max_retries, 3);
    }
}
