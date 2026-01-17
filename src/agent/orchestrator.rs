use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Agent role (for future config-based role selection)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum AgentRole {
    /// Planning (Gemini/Codex)
    Planner,
    /// Implementation (Claude)
    Executor,
}

/// Individual agent specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    pub command: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub description: String,
}

/// Orchestrator configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    /// Default planner
    pub default_planner: String,
    /// Default executor
    pub default_executor: String,
    /// Available planners
    pub planners: std::collections::HashMap<String, AgentSpec>,
    /// Available executors
    pub executors: std::collections::HashMap<String, AgentSpec>,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        let mut planners = std::collections::HashMap::new();
        planners.insert(
            "gemini".into(),
            AgentSpec {
                command: "gemini".into(),
                args: vec!["-y".into()],
                description: "Fast and cheap. Best for general tasks".into(),
            },
        );
        planners.insert(
            "codex".into(),
            AgentSpec {
                command: "codex".into(),
                args: vec![],
                description: "Strong reasoning. For complex architecture design".into(),
            },
        );

        let mut executors = std::collections::HashMap::new();
        executors.insert(
            "claude".into(),
            AgentSpec {
                command: "claude".into(),
                args: vec!["-p".into(), "--dangerously-skip-permissions".into()],
                description: "High code quality. Best for implementation".into(),
            },
        );

        Self {
            default_planner: "gemini".into(),
            default_executor: "claude".into(),
            planners,
            executors,
        }
    }
}

impl OrchestratorConfig {
    /// Load from config file
    pub fn load(hive_dir: &PathBuf) -> Result<Self> {
        let config_path = hive_dir.join("config.json");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .context("Failed to read config.json")?;
            let config: serde_json::Value = serde_json::from_str(&content)
                .context("Failed to parse config.json")?;

            // Load orchestrator section if exists
            if let Some(orch) = config.get("orchestrator") {
                return serde_json::from_value(orch.clone())
                    .context("Failed to parse orchestrator config");
            }
        }
        Ok(Self::default())
    }

    /// Get available planners
    pub fn available_planners(&self) -> Vec<&str> {
        self.planners.keys().map(|s| s.as_str()).collect()
    }

    /// Get available executors
    pub fn available_executors(&self) -> Vec<&str> {
        self.executors.keys().map(|s| s.as_str()).collect()
    }

    /// Get planner configuration
    #[allow(dead_code)]
    pub fn get_planner(&self, name: &str) -> Option<&AgentSpec> {
        self.planners.get(name)
    }

    /// Get executor configuration
    #[allow(dead_code)]
    pub fn get_executor(&self, name: &str) -> Option<&AgentSpec> {
        self.executors.get(name)
    }
}

/// Plan file manager
pub struct PlanManager {
    plans_dir: PathBuf,
}

impl PlanManager {
    pub fn new(hive_dir: PathBuf) -> Self {
        let plans_dir = hive_dir.join("plans");
        std::fs::create_dir_all(&plans_dir).ok();
        Self { plans_dir }
    }

    /// Get plan file path
    pub fn plan_path(&self, task_id: &str) -> PathBuf {
        self.plans_dir.join(format!("{}.md", task_id))
    }

    /// Check if plan file exists
    pub fn plan_file_exists(&self, task_id: &str) -> bool {
        self.plan_path(task_id).exists()
    }

    /// Load plan content
    pub fn load_plan(&self, task_id: &str) -> Result<String> {
        let path = self.plan_path(task_id);
        std::fs::read_to_string(&path)
            .context(format!("Failed to read plan: {}", path.display()))
    }

    /// Save plan content
    #[allow(dead_code)]
    pub fn save_plan(&self, task_id: &str, content: &str) -> Result<()> {
        let path = self.plan_path(task_id);
        std::fs::write(&path, content)
            .context(format!("Failed to write plan: {}", path.display()))
    }

    /// Create planning prompt
    pub fn create_planning_prompt(&self, task_title: &str, task_description: &str) -> String {
        format!(
            r#"Please create an implementation plan for the following task.

## Task
**Title**: {}
**Description**: {}

## Output Format
Please output in Markdown format as follows:

```markdown
# Implementation Plan: [Task Title]

## Overview
[Task purpose and goals]

## Implementation Steps
1. [Step 1]
   - Details
   - Affected files

2. [Step 2]
   ...

## Scope of Impact
- New files:
- Modified files:

## Test Strategy
- [Test 1]
- [Test 2]

## Notes and Risks
- [Note 1]
```
"#,
            task_title, task_description
        )
    }

    /// Create execution prompt with plan
    pub fn create_execution_prompt(&self, task_id: &str) -> Result<String> {
        let plan = self.load_plan(task_id)?;
        Ok(format!(
            r#"Please implement the code according to the following implementation plan.

{}

---
Follow the plan and proceed with implementation step by step.
After completing each step, verify it works before proceeding to the next step.
"#,
            plan
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ========================================
    // OrchestratorConfig Tests
    // ========================================

    #[test]
    fn test_default_config_plan_file_existsners() {
        let config = OrchestratorConfig::default();

        // Default planners: gemini, codex
        let planners = config.available_planners();
        assert!(planners.contains(&"gemini"));
        assert!(planners.contains(&"codex"));
        assert_eq!(planners.len(), 2);
    }

    #[test]
    fn test_default_config_has_executors() {
        let config = OrchestratorConfig::default();

        // Default executor: claude
        let executors = config.available_executors();
        assert!(executors.contains(&"claude"));
        assert_eq!(executors.len(), 1);
    }

    #[test]
    fn test_default_planner_and_executor() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.default_planner, "gemini");
        assert_eq!(config.default_executor, "claude");
    }

    #[test]
    fn test_get_planner() {
        let config = OrchestratorConfig::default();

        let gemini = config.get_planner("gemini");
        assert!(gemini.is_some());
        let gemini = gemini.unwrap();
        assert_eq!(gemini.command, "gemini");
        assert!(gemini.args.contains(&"-y".to_string()));

        let codex = config.get_planner("codex");
        assert!(codex.is_some());
        assert_eq!(codex.unwrap().command, "codex");

        let unknown = config.get_planner("unknown");
        assert!(unknown.is_none());
    }

    #[test]
    fn test_get_executor() {
        let config = OrchestratorConfig::default();

        let claude = config.get_executor("claude");
        assert!(claude.is_some());
        let claude = claude.unwrap();
        assert_eq!(claude.command, "claude");
        assert!(claude.args.contains(&"-p".to_string()));

        let unknown = config.get_executor("unknown");
        assert!(unknown.is_none());
    }

    #[test]
    fn test_agent_spec_has_description() {
        let config = OrchestratorConfig::default();

        let gemini = config.get_planner("gemini").unwrap();
        assert!(!gemini.description.is_empty());

        let claude = config.get_executor("claude").unwrap();
        assert!(!claude.description.is_empty());
    }

    #[test]
    fn test_load_returns_default_when_no_config() {
        let temp_dir = TempDir::new().unwrap();
        let config = OrchestratorConfig::load(&temp_dir.path().to_path_buf()).unwrap();

        // Should return default config
        assert_eq!(config.default_planner, "gemini");
        assert_eq!(config.default_executor, "claude");
    }

    #[test]
    fn test_load_from_config_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        // Write custom config
        let config_json = r#"{
            "orchestrator": {
                "default_planner": "codex",
                "default_executor": "claude",
                "planners": {
                    "codex": {
                        "command": "codex",
                        "args": ["--model", "o1"],
                        "description": "Custom codex"
                    }
                },
                "executors": {
                    "claude": {
                        "command": "claude",
                        "args": ["-p"],
                        "description": "Custom claude"
                    }
                }
            }
        }"#;
        std::fs::write(&config_path, config_json).unwrap();

        let config = OrchestratorConfig::load(&temp_dir.path().to_path_buf()).unwrap();
        assert_eq!(config.default_planner, "codex");

        let codex = config.get_planner("codex").unwrap();
        assert!(codex.args.contains(&"--model".to_string()));
    }

    // ========================================
    // PlanManager Tests
    // ========================================

    #[test]
    fn test_plan_manager_creates_plans_dir() {
        let temp_dir = TempDir::new().unwrap();
        let _manager = PlanManager::new(temp_dir.path().to_path_buf());

        let plans_dir = temp_dir.path().join("plans");
        assert!(plans_dir.exists());
    }

    #[test]
    fn test_plan_path() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlanManager::new(temp_dir.path().to_path_buf());

        let path = manager.plan_path("task-abc123");
        assert!(path.ends_with("task-abc123.md"));
        assert!(path.to_string_lossy().contains("plans"));
    }

    #[test]
    fn test_plan_file_exists_false_when_not_exists() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlanManager::new(temp_dir.path().to_path_buf());

        assert!(!manager.plan_file_exists("nonexistent-task"));
    }

    #[test]
    fn test_save_and_load_plan() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlanManager::new(temp_dir.path().to_path_buf());

        let task_id = "task-test123";
        let plan_content = "# Implementation Plan\n\n## Steps\n1. Do something";

        // Save
        manager.save_plan(task_id, plan_content).unwrap();

        // Has plan should return true
        assert!(manager.plan_file_exists(task_id));

        // Load
        let loaded = manager.load_plan(task_id).unwrap();
        assert_eq!(loaded, plan_content);
    }

    #[test]
    fn test_load_plan_error_when_not_exists() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlanManager::new(temp_dir.path().to_path_buf());

        let result = manager.load_plan("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_create_planning_prompt() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlanManager::new(temp_dir.path().to_path_buf());

        let prompt = manager.create_planning_prompt("Add login feature", "Implement OAuth login");

        assert!(prompt.contains("Add login feature"));
        assert!(prompt.contains("Implement OAuth login"));
        assert!(prompt.contains("Implementation Plan"));
        assert!(prompt.contains("## Overview"));
        assert!(prompt.contains("## Implementation Steps"));
        assert!(prompt.contains("## Scope of Impact"));
        assert!(prompt.contains("## Test Strategy"));
    }

    #[test]
    fn test_create_execution_prompt() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlanManager::new(temp_dir.path().to_path_buf());

        let task_id = "task-exec";
        let plan = "# Plan\n\n## Steps\n1. First step\n2. Second step";
        manager.save_plan(task_id, plan).unwrap();

        let prompt = manager.create_execution_prompt(task_id).unwrap();

        assert!(prompt.contains("First step"));
        assert!(prompt.contains("Second step"));
        assert!(prompt.contains("step by step"));
    }

    #[test]
    fn test_create_execution_prompt_error_when_no_plan() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlanManager::new(temp_dir.path().to_path_buf());

        let result = manager.create_execution_prompt("no-plan-task");
        assert!(result.is_err());
    }

    // ========================================
    // AgentRole Tests
    // ========================================

    #[test]
    fn test_agent_role_serialization() {
        // Test that AgentRole serializes correctly
        let planner = AgentRole::Planner;
        let executor = AgentRole::Executor;

        let planner_json = serde_json::to_string(&planner).unwrap();
        let executor_json = serde_json::to_string(&executor).unwrap();

        assert_eq!(planner_json, "\"planner\"");
        assert_eq!(executor_json, "\"executor\"");
    }

    #[test]
    fn test_agent_role_deserialization() {
        let planner: AgentRole = serde_json::from_str("\"planner\"").unwrap();
        let executor: AgentRole = serde_json::from_str("\"executor\"").unwrap();

        assert_eq!(planner, AgentRole::Planner);
        assert_eq!(executor, AgentRole::Executor);
    }
}
