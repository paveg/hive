use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Task status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Todo,
    /// Planner running (generating plan)
    Planning,
    /// Plan generated, awaiting user review
    PlanReview,
    /// Executor running (implementing code)
    InProgress,
    /// Implementation complete, awaiting code review
    Review,
    Done,
    Cancelled,
}

impl TaskStatus {
    /// Get display name for the status
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Todo => "Todo",
            Self::Planning => "Planning",
            Self::PlanReview => "Plan Review",
            Self::InProgress => "In Progress",
            Self::Review => "Review",
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
        }
    }

    /// Convert to column index
    /// Todo=0, Planning/PlanReview/InProgress=1, Review=2, Done=3
    pub fn to_column_index(&self) -> Option<usize> {
        match self {
            Self::Todo => Some(0),
            Self::Planning | Self::PlanReview | Self::InProgress => Some(1),
            Self::Review => Some(2),
            Self::Done => Some(3),
            Self::Cancelled => None,
        }
    }

    /// Get icon for the status
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Todo => "📋",
            Self::Planning => "🧠",
            Self::PlanReview => "📝",
            Self::InProgress => "🔨",
            Self::Review => "👀",
            Self::Done => "✅",
            Self::Cancelled => "❌",
        }
    }
}

/// Task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique ID
    pub id: String,
    /// Task title
    pub title: String,
    /// Task description
    pub description: String,
    /// Current status
    pub status: TaskStatus,
    /// Assigned planner (gemini, codex)
    pub planner: Option<String>,
    /// Assigned executor (claude)
    pub executor: Option<String>,
    /// Kept for backward compatibility (stores executor name)
    pub agent: Option<String>,
    /// Working branch name
    pub branch: Option<String>,
    /// Worktree path
    pub worktree: Option<String>,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Started timestamp
    pub started_at: Option<DateTime<Utc>>,
    /// Completed timestamp
    pub completed_at: Option<DateTime<Utc>>,
    /// Log file path
    pub output_log: Option<String>,
}

impl Task {
    /// Create a new task
    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        let id = format!("task-{}", Uuid::new_v4().to_string().split('-').next().unwrap());
        Self {
            id,
            title: title.into(),
            description: description.into(),
            status: TaskStatus::Todo,
            planner: None,
            executor: None,
            agent: None,
            branch: None,
            worktree: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            output_log: None,
        }
    }

    /// Change status
    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
        match status {
            TaskStatus::Planning | TaskStatus::InProgress => {
                if self.started_at.is_none() {
                    self.started_at = Some(Utc::now());
                }
            }
            TaskStatus::Done | TaskStatus::Cancelled => {
                self.completed_at = Some(Utc::now());
            }
            _ => {}
        }
    }

    /// Assign a planner
    pub fn assign_planner(&mut self, planner: impl Into<String>) {
        self.planner = Some(planner.into());
    }

    /// Assign an executor
    pub fn assign_executor(&mut self, executor: impl Into<String>, branch: impl Into<String>) {
        let exec = executor.into();
        self.executor = Some(exec.clone());
        self.agent = Some(exec); // backward compatibility
        self.branch = Some(branch.into());
    }

    /// Assign an agent (backward compatibility)
    #[allow(dead_code)]
    pub fn assign_agent(&mut self, agent: impl Into<String>, branch: impl Into<String>) {
        self.assign_executor(agent, branch);
    }

    /// Check if plan is approved (PlanReview or later status)
    #[allow(dead_code)]
    pub fn is_plan_approved(&self) -> bool {
        matches!(
            self.status,
            TaskStatus::PlanReview | TaskStatus::InProgress | TaskStatus::Review | TaskStatus::Done
        )
    }

    /// Check if forward transition is possible
    /// External conditions (e.g., plan file existence) must be checked separately
    pub fn can_advance(&self) -> Result<TaskStatus, &'static str> {
        match self.status {
            TaskStatus::Todo => {
                if self.planner.is_none() {
                    Err("Please assign a planner first")
                } else {
                    Ok(TaskStatus::Planning)
                }
            }
            TaskStatus::Planning => {
                // Plan file existence is checked externally
                Ok(TaskStatus::PlanReview)
            }
            TaskStatus::PlanReview => {
                if self.executor.is_none() {
                    Err("Please assign an executor first")
                } else {
                    Ok(TaskStatus::InProgress)
                }
            }
            TaskStatus::InProgress => Ok(TaskStatus::Review),
            TaskStatus::Review => Ok(TaskStatus::Done),
            TaskStatus::Done | TaskStatus::Cancelled => {
                Err("Cannot advance further")
            }
        }
    }

    /// Get retreat target status
    pub fn retreat_target(&self) -> Option<TaskStatus> {
        match self.status {
            TaskStatus::Todo | TaskStatus::Cancelled => None,
            TaskStatus::Planning => Some(TaskStatus::Todo),
            TaskStatus::PlanReview => Some(TaskStatus::Planning),
            // InProgress → Planning (skip PlanReview: revise plan)
            TaskStatus::InProgress => Some(TaskStatus::Planning),
            TaskStatus::Review => Some(TaskStatus::InProgress),
            TaskStatus::Done => Some(TaskStatus::Review),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // TaskStatus Tests
    // ========================================

    #[test]
    fn test_status_display_name() {
        assert_eq!(TaskStatus::Todo.display_name(), "Todo");
        assert_eq!(TaskStatus::Planning.display_name(), "Planning");
        assert_eq!(TaskStatus::PlanReview.display_name(), "Plan Review");
        assert_eq!(TaskStatus::InProgress.display_name(), "In Progress");
        assert_eq!(TaskStatus::Review.display_name(), "Review");
        assert_eq!(TaskStatus::Done.display_name(), "Done");
        assert_eq!(TaskStatus::Cancelled.display_name(), "Cancelled");
    }

    #[test]
    fn test_status_to_column_index() {
        // Todo → Column 0
        assert_eq!(TaskStatus::Todo.to_column_index(), Some(0));

        // Planning, PlanReview, InProgress → Column 1 (Progress)
        assert_eq!(TaskStatus::Planning.to_column_index(), Some(1));
        assert_eq!(TaskStatus::PlanReview.to_column_index(), Some(1));
        assert_eq!(TaskStatus::InProgress.to_column_index(), Some(1));

        // Review → Column 2
        assert_eq!(TaskStatus::Review.to_column_index(), Some(2));

        // Done → Column 3
        assert_eq!(TaskStatus::Done.to_column_index(), Some(3));

        // Cancelled → None (not displayed in kanban)
        assert_eq!(TaskStatus::Cancelled.to_column_index(), None);
    }

    #[test]
    fn test_status_icon() {
        assert_eq!(TaskStatus::Todo.icon(), "📋");
        assert_eq!(TaskStatus::Planning.icon(), "🧠");
        assert_eq!(TaskStatus::PlanReview.icon(), "📝");
        assert_eq!(TaskStatus::InProgress.icon(), "🔨");
        assert_eq!(TaskStatus::Review.icon(), "👀");
        assert_eq!(TaskStatus::Done.icon(), "✅");
        assert_eq!(TaskStatus::Cancelled.icon(), "❌");
    }

    #[test]
    fn test_status_default() {
        let status: TaskStatus = Default::default();
        assert_eq!(status, TaskStatus::Todo);
    }

    // ========================================
    // Task Creation Tests
    // ========================================

    #[test]
    fn test_task_creation() {
        let task = Task::new("Test task", "Description");
        assert!(task.id.starts_with("task-"));
        assert_eq!(task.title, "Test task");
        assert_eq!(task.description, "Description");
        assert_eq!(task.status, TaskStatus::Todo);
        assert!(task.planner.is_none());
        assert!(task.executor.is_none());
        assert!(task.agent.is_none());
        assert!(task.branch.is_none());
        assert!(task.worktree.is_none());
        assert!(task.started_at.is_none());
        assert!(task.completed_at.is_none());
    }

    #[test]
    fn test_task_id_uniqueness() {
        let task1 = Task::new("Task 1", "");
        let task2 = Task::new("Task 2", "");
        assert_ne!(task1.id, task2.id);
    }

    // ========================================
    // Status Transition Tests
    // ========================================

    #[test]
    fn test_status_transition_sets_started_at() {
        let mut task = Task::new("Test", "");
        assert!(task.started_at.is_none());

        // Planning should set started_at
        task.set_status(TaskStatus::Planning);
        assert!(task.started_at.is_some());
        let started = task.started_at;

        // InProgress should not change started_at
        task.set_status(TaskStatus::InProgress);
        assert_eq!(task.started_at, started);
    }

    #[test]
    fn test_status_transition_sets_completed_at() {
        let mut task = Task::new("Test", "");
        assert!(task.completed_at.is_none());

        task.set_status(TaskStatus::Done);
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn test_status_transition_cancelled_sets_completed_at() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::Cancelled);
        assert!(task.completed_at.is_some());
    }

    #[test]
    fn test_full_workflow_transition() {
        let mut task = Task::new("Feature", "Implement feature X");

        // Todo → Planning (Planner assigned)
        task.assign_planner("gemini");
        task.set_status(TaskStatus::Planning);
        assert_eq!(task.status, TaskStatus::Planning);
        assert!(task.started_at.is_some());

        // Planning → PlanReview (Plan generated)
        task.set_status(TaskStatus::PlanReview);
        assert_eq!(task.status, TaskStatus::PlanReview);

        // PlanReview → InProgress (Executor assigned)
        task.assign_executor("claude", "hive/feature-x");
        task.set_status(TaskStatus::InProgress);
        assert_eq!(task.status, TaskStatus::InProgress);

        // InProgress → Review (Implementation done)
        task.set_status(TaskStatus::Review);
        assert_eq!(task.status, TaskStatus::Review);

        // Review → Done (Approved)
        task.set_status(TaskStatus::Done);
        assert_eq!(task.status, TaskStatus::Done);
        assert!(task.completed_at.is_some());
    }

    // ========================================
    // Planner/Executor Assignment Tests
    // ========================================

    #[test]
    fn test_assign_planner() {
        let mut task = Task::new("Test", "");
        task.assign_planner("gemini");
        assert_eq!(task.planner, Some("gemini".into()));

        // Can reassign
        task.assign_planner("codex");
        assert_eq!(task.planner, Some("codex".into()));
    }

    #[test]
    fn test_assign_executor() {
        let mut task = Task::new("Test", "");
        task.assign_executor("claude", "hive/task-123");

        assert_eq!(task.executor, Some("claude".into()));
        assert_eq!(task.agent, Some("claude".into())); // backward compat
        assert_eq!(task.branch, Some("hive/task-123".into()));
    }

    #[test]
    fn test_assign_agent_backward_compat() {
        let mut task = Task::new("Test", "");
        task.assign_agent("claude", "branch-name");

        // assign_agent should work the same as assign_executor
        assert_eq!(task.executor, Some("claude".into()));
        assert_eq!(task.agent, Some("claude".into()));
        assert_eq!(task.branch, Some("branch-name".into()));
    }

    #[test]
    fn test_planner_executor_assignment() {
        let mut task = Task::new("Test", "");
        task.assign_planner("gemini");
        task.assign_executor("claude", "hive/task-123");

        assert_eq!(task.planner, Some("gemini".into()));
        assert_eq!(task.executor, Some("claude".into()));
        assert_eq!(task.agent, Some("claude".into())); // backward compat
    }

    // ========================================
    // has_plan Tests
    // ========================================

    #[test]
    fn test_is_plan_approved() {
        let mut task = Task::new("Test", "");

        // Todo and Planning should not have plan
        assert!(!task.is_plan_approved());

        task.set_status(TaskStatus::Planning);
        assert!(!task.is_plan_approved());

        // PlanReview and later should have plan
        task.set_status(TaskStatus::PlanReview);
        assert!(task.is_plan_approved());

        task.set_status(TaskStatus::InProgress);
        assert!(task.is_plan_approved());

        task.set_status(TaskStatus::Review);
        assert!(task.is_plan_approved());

        task.set_status(TaskStatus::Done);
        assert!(task.is_plan_approved());
    }

    #[test]
    fn test_cancelled_task_has_no_plan() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::Cancelled);
        assert!(!task.is_plan_approved());
    }

    // ========================================
    // JSON Serialization Tests
    // ========================================

    #[test]
    fn test_status_json_serialization() {
        // Verify snake_case serialization
        assert_eq!(serde_json::to_string(&TaskStatus::Todo).unwrap(), "\"todo\"");
        assert_eq!(serde_json::to_string(&TaskStatus::Planning).unwrap(), "\"planning\"");
        assert_eq!(serde_json::to_string(&TaskStatus::PlanReview).unwrap(), "\"plan_review\"");
        assert_eq!(serde_json::to_string(&TaskStatus::InProgress).unwrap(), "\"in_progress\"");
        assert_eq!(serde_json::to_string(&TaskStatus::Review).unwrap(), "\"review\"");
        assert_eq!(serde_json::to_string(&TaskStatus::Done).unwrap(), "\"done\"");
        assert_eq!(serde_json::to_string(&TaskStatus::Cancelled).unwrap(), "\"cancelled\"");
    }

    #[test]
    fn test_status_json_deserialization() {
        assert_eq!(serde_json::from_str::<TaskStatus>("\"todo\"").unwrap(), TaskStatus::Todo);
        assert_eq!(serde_json::from_str::<TaskStatus>("\"planning\"").unwrap(), TaskStatus::Planning);
        assert_eq!(serde_json::from_str::<TaskStatus>("\"plan_review\"").unwrap(), TaskStatus::PlanReview);
        assert_eq!(serde_json::from_str::<TaskStatus>("\"in_progress\"").unwrap(), TaskStatus::InProgress);
        assert_eq!(serde_json::from_str::<TaskStatus>("\"review\"").unwrap(), TaskStatus::Review);
        assert_eq!(serde_json::from_str::<TaskStatus>("\"done\"").unwrap(), TaskStatus::Done);
        assert_eq!(serde_json::from_str::<TaskStatus>("\"cancelled\"").unwrap(), TaskStatus::Cancelled);
    }

    #[test]
    fn test_task_json_round_trip() {
        let mut task = Task::new("Test Task", "Description");
        task.assign_planner("gemini");
        task.assign_executor("claude", "hive/test");
        task.set_status(TaskStatus::InProgress);

        let json = serde_json::to_string(&task).unwrap();
        let deserialized: Task = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.title, task.title);
        assert_eq!(deserialized.description, task.description);
        assert_eq!(deserialized.status, task.status);
        assert_eq!(deserialized.planner, task.planner);
        assert_eq!(deserialized.executor, task.executor);
        assert_eq!(deserialized.branch, task.branch);
    }

    // ========================================
    // Timestamp Tests
    // ========================================

    #[test]
    fn test_created_at_is_set_on_creation() {
        let before = Utc::now();
        let task = Task::new("Test", "");
        let after = Utc::now();

        assert!(task.created_at >= before);
        assert!(task.created_at <= after);
    }

    #[test]
    fn test_started_at_only_set_once() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::Planning);
        let first_started = task.started_at;

        // Transition through multiple statuses
        task.set_status(TaskStatus::PlanReview);
        task.set_status(TaskStatus::InProgress);

        // started_at should not change
        assert_eq!(task.started_at, first_started);
    }

    // ========================================
    // Edge Cases
    // ========================================

    #[test]
    fn test_empty_title_and_description() {
        let task = Task::new("", "");
        assert_eq!(task.title, "");
        assert_eq!(task.description, "");
    }

    #[test]
    fn test_unicode_in_task() {
        let task = Task::new("日本語タスク 🎉", "説明文 with émojis 🚀");
        assert_eq!(task.title, "日本語タスク 🎉");
        assert_eq!(task.description, "説明文 with émojis 🚀");

        // Verify JSON round-trip works with unicode
        let json = serde_json::to_string(&task).unwrap();
        let deserialized: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, task.title);
        assert_eq!(deserialized.description, task.description);
    }

    #[test]
    fn test_special_characters_in_task() {
        let task = Task::new(
            "Task with \"quotes\" and 'apostrophes'",
            "Description with\nnewlines\tand\ttabs"
        );

        let json = serde_json::to_string(&task).unwrap();
        let deserialized: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.title, task.title);
        assert_eq!(deserialized.description, task.description);
    }

    #[test]
    fn test_long_task_content() {
        let long_title = "A".repeat(1000);
        let long_description = "B".repeat(10000);
        let task = Task::new(&long_title, &long_description);

        assert_eq!(task.title.len(), 1000);
        assert_eq!(task.description.len(), 10000);
    }

    // ========================================
    // Status Transition Validation Tests
    // ========================================

    #[test]
    fn test_can_advance_from_todo_requires_planner() {
        let task = Task::new("Test", "");
        let result = task.can_advance();
        match result {
            Err(msg) => assert!(msg.contains("planner"), "Expected 'planner' in message: {}", msg),
            Ok(_) => panic!("Expected Err, got Ok"),
        }
    }

    #[test]
    fn test_can_advance_from_todo_with_planner() {
        let mut task = Task::new("Test", "");
        task.assign_planner("gemini");
        let result = task.can_advance();
        assert_eq!(result.unwrap(), TaskStatus::Planning);
    }

    #[test]
    fn test_can_advance_from_planning() {
        let mut task = Task::new("Test", "");
        task.assign_planner("gemini");
        task.set_status(TaskStatus::Planning);
        // Planning → PlanReview is OK (plan file is checked separately)
        assert_eq!(task.can_advance().unwrap(), TaskStatus::PlanReview);
    }

    #[test]
    fn test_can_advance_from_plan_review_requires_executor() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::PlanReview);
        let result = task.can_advance();
        match result {
            Err(msg) => assert!(msg.contains("executor"), "Expected 'executor' in message: {}", msg),
            Ok(_) => panic!("Expected Err, got Ok"),
        }
    }

    #[test]
    fn test_can_advance_from_plan_review_with_executor() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::PlanReview);
        task.assign_executor("claude", "branch");
        assert_eq!(task.can_advance().unwrap(), TaskStatus::InProgress);
    }

    #[test]
    fn test_can_advance_from_in_progress() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::InProgress);
        assert_eq!(task.can_advance().unwrap(), TaskStatus::Review);
    }

    #[test]
    fn test_can_advance_from_review() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::Review);
        assert_eq!(task.can_advance().unwrap(), TaskStatus::Done);
    }

    #[test]
    fn test_can_advance_from_done_fails() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::Done);
        assert!(task.can_advance().is_err());
    }

    #[test]
    fn test_retreat_target_from_todo() {
        let task = Task::new("Test", "");
        assert_eq!(task.retreat_target(), None);
    }

    #[test]
    fn test_retreat_target_from_planning() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::Planning);
        assert_eq!(task.retreat_target(), Some(TaskStatus::Todo));
    }

    #[test]
    fn test_retreat_target_from_plan_review() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::PlanReview);
        assert_eq!(task.retreat_target(), Some(TaskStatus::Planning));
    }

    #[test]
    fn test_retreat_target_from_in_progress_skips_plan_review() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::InProgress);
        // InProgress → Planning (revise plan, skip PlanReview)
        assert_eq!(task.retreat_target(), Some(TaskStatus::Planning));
    }

    #[test]
    fn test_retreat_target_from_review() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::Review);
        assert_eq!(task.retreat_target(), Some(TaskStatus::InProgress));
    }

    #[test]
    fn test_retreat_target_from_done() {
        let mut task = Task::new("Test", "");
        task.set_status(TaskStatus::Done);
        assert_eq!(task.retreat_target(), Some(TaskStatus::Review));
    }
}
