use super::types::Generated;
use crate::error::{AppError, Result};
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;
fn norm(s: &str) -> String {
    s.trim().nfkc().collect::<String>().to_lowercase()
}
fn clean(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_control())
}
pub fn validate(x: &Generated) -> Result<()> {
    if !clean(x.translation.trim()) || x.translation.len() > 1000 {
        return Err(AppError::Validation(
            "empty or oversized translation".into(),
        ));
    }
    if x.blocks.is_empty() || x.blocks.len() > 50 {
        return Err(AppError::Validation("block count must be 1..50".into()));
    }
    for (i, b) in x.blocks.iter().enumerate() {
        if b.position != i {
            return Err(AppError::Validation("positions must be sequential".into()));
        }
        if !clean(b.correct.trim()) || b.correct.len() > 200 || b.distractors.len() != 4 {
            return Err(AppError::Validation(
                "each block requires one answer and four distractors".into(),
            ));
        }
        let mut seen = HashSet::from([norm(&b.correct)]);
        for d in &b.distractors {
            if !clean(d.trim()) || d.len() > 200 || !seen.insert(norm(d)) {
                return Err(AppError::Validation(
                    "options must be non-empty and unique".into(),
                ));
            }
        }
    }
    let joined = x
        .blocks
        .iter()
        .map(|b| b.correct.trim())
        .collect::<Vec<_>>()
        .join(" ");
    if norm(&joined) != norm(&x.translation) {
        return Err(AppError::Validation(
            "blocks do not reconstruct translation".into(),
        ));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::types::*;
    #[test]
    fn rejects_duplicate() {
        let x = Generated {
            source_text: "x".into(),
            target_language: "German".into(),
            translation: "Ich".into(),
            blocks: vec![GeneratedBlock {
                position: 0,
                correct: "Ich".into(),
                distractors: vec!["ich".into(), "Du".into(), "Er".into(), "Wir".into()],
                hint: None,
            }],
        };
        assert!(validate(&x).is_err())
    }

    #[test]
    fn accepts_sequential_positions() {
        let x = Generated {
            source_text: "Меня зовут Фия".into(),
            target_language: "English".into(),
            translation: "My name is Phia".into(),
            blocks: vec![
                GeneratedBlock {
                    position: 0,
                    correct: "My name".into(),
                    distractors: vec![
                        "Your name".into(),
                        "His name".into(),
                        "Our name".into(),
                        "Their name".into(),
                    ],
                    hint: None,
                },
                GeneratedBlock {
                    position: 1,
                    correct: "is Phia".into(),
                    distractors: vec![
                        "was Phia".into(),
                        "is Maria".into(),
                        "are Phia".into(),
                        "is Sofia".into(),
                    ],
                    hint: None,
                },
            ],
        };
        assert!(validate(&x).is_ok())
    }
}
