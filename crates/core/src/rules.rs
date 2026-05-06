use crate::entities::{AttributionRule, RuleType};

pub struct RulesEngine {
    rules: Vec<AttributionRule>,
}

impl RulesEngine {
    pub fn new(mut rules: Vec<AttributionRule>) -> Self {
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        Self { rules }
    }

    pub fn match_event(&self, source: &str, text: &str) -> Option<i64> {
        for rule in &self.rules {
            if rule.source != "*" && rule.source != source { continue; }
            match rule.rule_type {
                RuleType::Channel | RuleType::Label | RuleType::Path => {
                    if text.contains(&rule.pattern) { return Some(rule.billing_project_id); }
                }
                RuleType::Keyword => {
                    if text.to_lowercase().contains(&rule.pattern.to_lowercase()) {
                        return Some(rule.billing_project_id);
                    }
                }
                RuleType::Llm => {}
            }
        }
        None
    }

    pub fn needs_llm_review(&self, source: &str, text: &str) -> bool {
        self.match_event(source, text).is_none()
            && self.rules.iter().any(|r| r.rule_type == RuleType::Llm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn rule(project_id: i64, source: &str, rtype: RuleType, pattern: &str, priority: i32) -> AttributionRule {
        AttributionRule {
            id: None, billing_project_id: project_id, source: source.into(),
            rule_type: rtype, pattern: pattern.into(), priority, created_at: Utc::now(),
        }
    }

    #[test]
    fn channel_rule_matches() {
        let engine = RulesEngine::new(vec![rule(1, "slack", RuleType::Channel, "#project-alpha", 10)]);
        assert_eq!(engine.match_event("slack", "#project-alpha discussion"), Some(1));
    }

    #[test]
    fn keyword_is_case_insensitive() {
        let engine = RulesEngine::new(vec![rule(2, "*", RuleType::Keyword, "chronos", 5)]);
        assert_eq!(engine.match_event("gmail", "Working on CHRONOS today"), Some(2));
    }

    #[test]
    fn higher_priority_wins() {
        let engine = RulesEngine::new(vec![
            rule(1, "*", RuleType::Keyword, "project", 5),
            rule(2, "slack", RuleType::Channel, "#project-beta", 10),
        ]);
        assert_eq!(engine.match_event("slack", "#project-beta update"), Some(2));
    }

    #[test]
    fn no_match_returns_none() {
        let engine = RulesEngine::new(vec![rule(1, "slack", RuleType::Channel, "#project-alpha", 10)]);
        assert_eq!(engine.match_event("gmail", "lunch plans"), None);
    }

    #[test]
    fn path_rule_matches_project_dir() {
        let engine = RulesEngine::new(vec![rule(1, "claude", RuleType::Path, "chronos", 10)]);
        assert_eq!(engine.match_event("claude", "session in ~/Documents/chronos"), Some(1));
    }

    #[test]
    fn source_filter_scopes_rules() {
        let engine = RulesEngine::new(vec![rule(1, "slack", RuleType::Keyword, "deploy", 5)]);
        assert_eq!(engine.match_event("gmail", "deploy notification"), None);
        assert_eq!(engine.match_event("slack", "deploy notification"), Some(1));
    }

    #[test]
    fn llm_fallback_flags_review() {
        let engine = RulesEngine::new(vec![
            rule(1, "slack", RuleType::Channel, "#known", 10),
            rule(1, "*", RuleType::Llm, "", 0),
        ]);
        assert!(!engine.needs_llm_review("slack", "#known channel"));
        assert!(engine.needs_llm_review("gmail", "random email"));
    }
}
