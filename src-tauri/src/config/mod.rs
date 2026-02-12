// Configuration management for PDF.dk Desktop

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Config directory not found")]
    NoConfigDir,
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
}

/// Saved authentication credentials
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    pub token: Option<String>,
    pub email: Option<String>,
    pub password: Option<String>,
}

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: u32,
    pub general: GeneralSettings,
    pub tools: Vec<ToolConfig>,
    #[serde(default)]
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettings {
    pub start_on_login: bool,
    pub start_minimized: bool,
    pub show_notifications: bool,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfig {
    pub id: String,
    pub enabled: bool,
    pub folder_path: Option<String>,
    pub output_mode: OutputMode,
    pub options: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    SameFolder,
    Subfolder,
    Custom(String),
}

/// Definition of an available tool (for UI display)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub id: String,
    pub name: String,
    pub name_da: String,
    pub description: String,
    pub description_da: String,
    pub api_endpoint: String,
    pub icon: String,
    pub has_options: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            general: GeneralSettings {
                start_on_login: true,
                start_minimized: true,
                show_notifications: true,
                language: "da".to_string(),
            },
            tools: vec![],
        }
    }
}

impl AppConfig {
    pub fn enable_tool(&mut self, tool_id: &str, folder_path: &str) -> Result<(), ConfigError> {
        // Verify tool exists
        let available = get_available_tools();
        if !available.iter().any(|t| t.id == tool_id) {
            return Err(ConfigError::ToolNotFound(tool_id.to_string()));
        }

        // Create folder if it doesn't exist
        let path = PathBuf::from(folder_path);
        if !path.exists() {
            fs::create_dir_all(&path)?;
        }

        // Create "Processed" subfolder
        let processed_path = path.join("Processed");
        if !processed_path.exists() {
            fs::create_dir_all(&processed_path)?;
        }

        // Update or add tool config
        if let Some(tool) = self.tools.iter_mut().find(|t| t.id == tool_id) {
            tool.enabled = true;
            tool.folder_path = Some(folder_path.to_string());
        } else {
            self.tools.push(ToolConfig {
                id: tool_id.to_string(),
                enabled: true,
                folder_path: Some(folder_path.to_string()),
                output_mode: OutputMode::Subfolder,
                options: serde_json::json!({}),
            });
        }

        Ok(())
    }

    pub fn disable_tool(&mut self, tool_id: &str) {
        if let Some(tool) = self.tools.iter_mut().find(|t| t.id == tool_id) {
            tool.enabled = false;
        }
    }

    pub fn get_enabled_tools(&self) -> Vec<&ToolConfig> {
        self.tools.iter().filter(|t| t.enabled).collect()
    }
}

/// Get the config file path
fn get_config_path() -> Result<PathBuf, ConfigError> {
    let config_dir = dirs::config_dir().ok_or(ConfigError::NoConfigDir)?;
    let app_config_dir = config_dir.join("dk.pdf.desktop");

    if !app_config_dir.exists() {
        fs::create_dir_all(&app_config_dir)?;
    }

    Ok(app_config_dir.join("config.json"))
}

/// Load configuration from disk
pub fn load_config() -> Result<AppConfig, ConfigError> {
    let path = get_config_path()?;

    if path.exists() {
        let content = fs::read_to_string(&path)?;
        let config: AppConfig = serde_json::from_str(&content)?;
        Ok(config)
    } else {
        Ok(AppConfig::default())
    }
}

/// Save configuration to disk
pub fn save_config(config: &AppConfig) -> Result<(), ConfigError> {
    let path = get_config_path()?;
    let content = serde_json::to_string_pretty(config)?;
    fs::write(&path, content)?;
    Ok(())
}

/// Get the default base folder path
pub fn get_default_base_folder() -> PathBuf {
    dirs::document_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("PDF.dk")
}

/// Get list of available tools
/// Starting with just Compress and Outline as requested
pub fn get_available_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            id: "compress".to_string(),
            name: "Compress PDF".to_string(),
            name_da: "Komprimer PDF".to_string(),
            description: "Reduce PDF file size while maintaining quality".to_string(),
            description_da: "Reducer PDF filstørrelse og bevar kvaliteten".to_string(),
            api_endpoint: "compress".to_string(),
            icon: "compress".to_string(),
            has_options: true,
        },
        ToolDefinition {
            id: "outline".to_string(),
            name: "Outline Fonts".to_string(),
            name_da: "Konverter Skrifttyper".to_string(),
            description: "Convert text to vector outlines for printing".to_string(),
            description_da: "Konverter tekst til vektorstier til tryk".to_string(),
            api_endpoint: "outline".to_string(),
            icon: "text".to_string(),
            has_options: false,
        },
        ToolDefinition {
            id: "pdf-to-word".to_string(),
            name: "PDF to Word".to_string(),
            name_da: "PDF til Word".to_string(),
            description: "Convert PDF to editable Word document".to_string(),
            description_da: "Konverter PDF til redigerbart Word-dokument".to_string(),
            api_endpoint: "pdf-to-word".to_string(),
            icon: "file-word".to_string(),
            has_options: false,
        },
        ToolDefinition {
            id: "pdf-to-excel".to_string(),
            name: "PDF to Excel".to_string(),
            name_da: "PDF til Excel".to_string(),
            description: "Convert PDF tables to Excel spreadsheet".to_string(),
            description_da: "Konverter PDF-tabeller til Excel-regneark".to_string(),
            api_endpoint: "pdf-to-excel".to_string(),
            icon: "file-excel".to_string(),
            has_options: false,
        },
        ToolDefinition {
            id: "pdf-to-jpg".to_string(),
            name: "PDF to JPG".to_string(),
            name_da: "PDF til JPG".to_string(),
            description: "Convert PDF pages to JPG images".to_string(),
            description_da: "Konverter PDF-sider til JPG-billeder".to_string(),
            api_endpoint: "pdf-to-jpg".to_string(),
            icon: "image".to_string(),
            has_options: false,
        },
        ToolDefinition {
            id: "rotate".to_string(),
            name: "Rotate PDF".to_string(),
            name_da: "Roter PDF".to_string(),
            description: "Rotate PDF pages 90°, 180°, or 270°".to_string(),
            description_da: "Roter PDF-sider 90°, 180° eller 270°".to_string(),
            api_endpoint: "rotate".to_string(),
            icon: "rotate".to_string(),
            has_options: true,
        },
        ToolDefinition {
            id: "unlock".to_string(),
            name: "Unlock PDF".to_string(),
            name_da: "Lås PDF Op".to_string(),
            description: "Remove password protection from PDF".to_string(),
            description_da: "Fjern adgangskodebeskyttelse fra PDF".to_string(),
            api_endpoint: "unlock".to_string(),
            icon: "unlock".to_string(),
            has_options: false,
        },
        ToolDefinition {
            id: "ocr".to_string(),
            name: "OCR PDF".to_string(),
            name_da: "OCR PDF".to_string(),
            description: "Make scanned PDFs searchable with OCR".to_string(),
            description_da: "Gør scannede PDF'er søgbare med OCR".to_string(),
            api_endpoint: "ocr".to_string(),
            icon: "scan".to_string(),
            has_options: true,
        },
        ToolDefinition {
            id: "bleed".to_string(),
            name: "Add Bleed".to_string(),
            name_da: "Tilføj Skæremærker".to_string(),
            description: "Add bleed margins for professional printing".to_string(),
            description_da: "Tilføj skæremærker til professionelt tryk".to_string(),
            api_endpoint: "bleed".to_string(),
            icon: "expand".to_string(),
            has_options: true,
        },
    ]
}
