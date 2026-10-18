use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    pub name: String,
    pub domain: String,
    pub stack: String,
    pub pages: Vec<Page>,
    pub redirects: Vec<Redirect>,
    pub footer_links: Vec<Link>,
    pub integrations: Vec<Integration>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Page {
    pub id: String,
    pub name: String,
    pub route: String,
    pub title: String,
    pub desc: String,
    pub nav: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub secs: Vec<Section>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Section {
    pub id: String,
    pub code: String,
    pub variant: String,
    pub label: String,
    pub formula: String,
    pub p: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Redirect {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Link {
    pub label: String,
    pub href: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Integration {
    pub id: String,
    pub name: String,
    pub via: String,
    pub env: Vec<String>,
}

// Structured operations will be added with stack adapters. Until then, edits
// review the complete source file; no guessed adapter can destroy custom blocks.
