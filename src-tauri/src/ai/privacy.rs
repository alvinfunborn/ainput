use std::collections::{HashMap, HashSet};
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::Mutex;

use crate::config;

// A thread-safe, lazily-initialized cache for compiled regular expressions.
static RE_CACHE: Lazy<Mutex<HashMap<String, Regex>>> = Lazy::new(|| Mutex::new(HashMap::new()));

// Holds the anonymized text and the mapping to restore it.
pub struct AnonymizedData {
    pub text: String,
    pub mapping: HashMap<String, String>,
}

// Anonymizes sensitive data in a given text based on regex rules in the config.
pub fn anonymize(text: &str) -> AnonymizedData {
    let privacy_config = &config::get_config().unwrap().privacy;

    // If disabled or no rules, return the original text.
    if !privacy_config.enable || privacy_config.rules.is_empty() {
        return AnonymizedData {
            text: text.to_string(),
            mapping: HashMap::new(),
        };
    }

    let mut anonymized_text = text.to_string();
    let mut mapping = HashMap::new();
    let mut placeholder_index = 1;

    for rule in &privacy_config.rules {
        // Clone the regex from the cache to release the lock quickly.
        let re = {
            let mut cache = RE_CACHE.lock().unwrap();
            cache.entry(rule.clone())
                 .or_insert_with(|| Regex::new(rule).expect("Invalid regex in config.toml"))
                 .clone()
        };

        // Collect all unique matched strings to avoid redundant processing.
        let matches: HashSet<String> = re.find_iter(&anonymized_text).map(|m| m.as_str().to_string()).collect();

        for mat in matches {
            let placeholder = format!("[PRIVATE_{}]", placeholder_index);
            // Replace all occurrences of the matched string.
            anonymized_text = anonymized_text.replace(&mat, &placeholder);
            mapping.insert(placeholder, mat);
            placeholder_index += 1;
        }
    }

    AnonymizedData {
        text: anonymized_text,
        mapping,
    }
}

// De-anonymizes text using the provided mapping.
pub fn deanonymize(text: &str, mapping: &HashMap<String, String>) -> String {
    if mapping.is_empty() {
        return text.to_string();
    }
    
    let mut deanonymized_text = text.to_string();
    for (placeholder, original_text) in mapping {
        deanonymized_text = deanonymized_text.replace(placeholder, original_text);
    }
    deanonymized_text
} 