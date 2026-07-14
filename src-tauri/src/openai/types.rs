use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Generated {
    pub source_text: String,
    pub target_language: String,
    pub translation: String,
    pub blocks: Vec<GeneratedBlock>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GeneratedBlock {
    pub position: usize,
    pub correct: String,
    pub distractors: Vec<String>,
    pub hint: Option<String>,
}
