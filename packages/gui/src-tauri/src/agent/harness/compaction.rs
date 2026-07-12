use crate::agent::harness::types::FileInfo;
use std::path::Path;
use async_trait::async_trait;
use tokio::fs;

#[derive(Debug)]
pub enum CompactionError {
    TokenizerInit(String),
}

/// A token estimator that uses tiktoken to approximate token counts.
pub struct TiktokenEstimator {
    /// The tokenizer for the target model.
    encoder: &'static tiktoken_rs::CoreBPE,
}

impl TiktokenEstimator {
    pub fn new(model_id: &str) -> Result<Self, CompactionError> {
        let encoder = tiktoken_rs::get_bpe_from_model(model_id)
            .map_err(|e| CompactionError::TokenizerInit(e.to_string()))?;
        Ok(Self { encoder })
    }

    pub fn estimate(&self, text: &str) -> usize {
        self.encoder.encode_with_special_tokens(text).len()
    }
}

/// A budget tracker tracks the current usage and what's allowed.
pub struct MessageBudget {
    /// Max allowed in the budget.
    pub max_tokens: usize,
    /// Current budget usage.
    pub current_usage: usize,
}

impl MessageBudget {
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            current_usage: 0,
        }
    }

    /// Can this message fit?
    pub fn can_fit(&self, tokens: usize) -> bool {
        self.current_usage + tokens <= self.max_tokens
    }

    /// Use tokens if they fit.
    /// Returns `true` on success.
    pub fn use_tokens(&mut self, tokens: usize) -> bool {
        if self.can_fit(tokens) {
            self.current_usage += tokens;
            true
        } else {
            false
        }
    }
}

pub struct CompactionFileInfo {
    pub path: String,
    pub mtime: i64,
    pub tokens: usize,
    pub content: String,
}

/// A compaction policy decides what gets compacted.
pub trait CompactionPolicy {
    /// Compact messages to fit under budget.
    fn compact(&self, budget: &mut MessageBudget, files: Vec<CompactionFileInfo>) -> Vec<CompactionFileInfo>;
}

/// Naive oldest-first compaction: drop oldest messages until under budget.
pub struct NaiveCompactor;

impl CompactionPolicy for NaiveCompactor {
    fn compact(&self, budget: &mut MessageBudget, files: Vec<CompactionFileInfo>) -> Vec<CompactionFileInfo> {
        let mut sorted = files;
        sorted.sort_by_key(|a| a.mtime);
        sorted.into_iter().filter(|f| budget.use_tokens(f.tokens)).collect()
    }
}
