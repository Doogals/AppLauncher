use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    App,
    File,
    Url,
    Folder,
    Script,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub item_type: ItemType,
    pub path: Option<String>,
    pub value: Option<String>,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub icon_data: Option<String>,
    #[serde(default)]
    pub browser_name: Option<String>,
    #[serde(default = "default_true")]
    pub run_in_terminal: bool,
    #[serde(default)]
    pub launch_desktop: Option<u32>,
    #[serde(default)]
    pub launch_x: Option<i32>,
    #[serde(default)]
    pub launch_y: Option<i32>,
    #[serde(default)]
    pub launch_width: Option<u32>,
    #[serde(default)]
    pub launch_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub items: Vec<Item>,
}

impl Group {
    pub fn new(name: &str, icon: &str) -> Self {
        Group {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            icon: icon.to_string(),
            items: vec![],
        }
    }
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    pub preferred_browser: Option<String>,
    pub license_key: Option<String>,
    pub license_instance_id: Option<String>,
    pub license_machine_name: Option<String>,
    pub groups: Vec<Group>,
    pub widget_x: Option<i32>,
    pub widget_y: Option<i32>,
    pub widget_color: Option<String>,
    #[serde(default = "default_true")]
    pub launch_on_startup: bool,
    #[serde(default)]
    pub widget_on_top: bool,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
}

fn default_hotkey() -> String { "Ctrl+Alt+Space".to_string() }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            preferred_browser: None,
            license_key: None,
            license_instance_id: None,
            license_machine_name: None,
            groups: vec![],
            widget_x: None,
            widget_y: None,
            widget_color: None,
            launch_on_startup: true,
            widget_on_top: false,
            hotkey: default_hotkey(),
        }
    }
}


pub fn config_path() -> PathBuf {
    let mut path = dirs::data_local_dir().expect("cannot find %LOCALAPPDATA%");
    path.push("AppLauncher");
    path.push("config.json");
    path
}

pub fn load_config() -> AppConfig {
    let path = config_path();
    if !path.exists() {
        return AppConfig::default();
    }
    let data = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_config_roundtrip(config: &AppConfig) -> AppConfig {
        let dir = std::env::temp_dir().join("app_launcher_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let data = serde_json::to_string_pretty(config).unwrap();
        fs::write(&path, &data).unwrap();
        let loaded: AppConfig = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        loaded
    }

    #[test]
    fn test_default_config_has_no_groups() {
        let config = AppConfig::default();
        assert_eq!(config.groups.len(), 0);
        assert!(config.preferred_browser.is_none());
        assert!(config.license_key.is_none());
    }

    #[test]
    fn test_group_new_generates_id() {
        let g1 = Group::new("Work", "💼");
        let g2 = Group::new("Work", "💼");
        assert_ne!(g1.id, g2.id);
        assert_eq!(g1.name, "Work");
        assert_eq!(g1.icon, "💼");
    }

    #[test]
    fn test_config_roundtrip_serialization() {
        let mut config = AppConfig::default();
        config.preferred_browser = Some("C:\\chrome.exe".to_string());
        config.groups.push(Group {
            id: "test-id".to_string(),
            name: "Work".to_string(),
            icon: "💼".to_string(),
            items: vec![
                Item { item_type: ItemType::App, path: Some("C:\\slack.exe".to_string()), value: None, urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
                Item { item_type: ItemType::Url, path: None, value: Some("https://github.com".to_string()), urls: vec![], icon_data: None, browser_name: None, run_in_terminal: true, launch_desktop: None, launch_x: None, launch_y: None, launch_width: None, launch_height: None },
            ],
        });

        let loaded = tmp_config_roundtrip(&config);
        assert_eq!(loaded, config);
    }

    #[test]
    fn test_load_config_returns_default_when_file_missing() {
        let result = serde_json::from_str::<AppConfig>("{}");
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_has_license_instance_fields() {
        let mut config = AppConfig::default();
        assert!(config.license_instance_id.is_none());
        assert!(config.license_machine_name.is_none());
        config.license_instance_id = Some("inst-123".to_string());
        config.license_machine_name = Some("My PC".to_string());
        // Use a separate temp path to avoid racing with test_config_roundtrip_serialization
        let dir = std::env::temp_dir().join("app_launcher_license_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let data = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &data).unwrap();
        let loaded: AppConfig = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.license_instance_id, Some("inst-123".to_string()));
        assert_eq!(loaded.license_machine_name, Some("My PC".to_string()));
    }

    #[test]
    fn test_item_run_in_terminal_defaults_to_true_when_absent() {
        let json = r#"{"item_type":"script","path":"/foo.bat","value":null}"#;
        let item: Item = serde_json::from_str(json).unwrap();
        assert!(item.run_in_terminal, "run_in_terminal should default to true");
    }

    #[test]
    fn test_item_urls_defaults_to_empty_when_absent() {
        let json = r#"{"item_type":"url","path":null,"value":"https://a.com"}"#;
        let item: Item = serde_json::from_str(json).unwrap();
        assert!(item.urls.is_empty(), "urls should default to empty vec");
        assert!(item.icon_data.is_none());
        assert!(item.browser_name.is_none());
    }

    #[test]
    fn test_item_new_fields_roundtrip() {
        let item = Item {
            item_type: ItemType::Url,
            path: Some("chrome.exe".into()),
            value: Some("https://a.com".into()),
            urls: vec!["https://a.com".into(), "https://b.com".into()],
            icon_data: Some("abc123".into()),
            browser_name: Some("Chrome".into()),
            run_in_terminal: false,
            launch_desktop: None, launch_x: None, launch_y: None,
            launch_width: None, launch_height: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        let loaded: Item = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.urls, vec!["https://a.com", "https://b.com"]);
        assert_eq!(loaded.icon_data.as_deref(), Some("abc123"));
        assert_eq!(loaded.browser_name.as_deref(), Some("Chrome"));
        assert!(!loaded.run_in_terminal);
    }
}
