use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::{Task, TaskStatus};

/// Task persistence handler
pub struct TaskStore {
    /// Path to .hive directory
    hive_dir: PathBuf,
}

impl TaskStore {
    /// Create a new TaskStore
    pub fn new(project_root: impl Into<PathBuf>) -> Result<Self> {
        let hive_dir = project_root.into().join(".hive");

        // Create .hive directory if it doesn't exist
        if !hive_dir.exists() {
            fs::create_dir_all(&hive_dir).context("Failed to create .hive directory")?;
            fs::create_dir_all(hive_dir.join("plans")).context("Failed to create plans dir")?;
            fs::create_dir_all(hive_dir.join("worktrees")).context("Failed to create worktrees dir")?;
            fs::create_dir_all(hive_dir.join("logs")).context("Failed to create logs dir")?;
        }

        Ok(Self { hive_dir })
    }

    /// Get path to tasks.json
    fn tasks_file(&self) -> PathBuf {
        self.hive_dir.join("tasks.json")
    }

    /// Load all tasks
    pub fn load(&self) -> Result<Vec<Task>> {
        let path = self.tasks_file();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&path).context("Failed to read tasks.json")?;
        let tasks: Vec<Task> = serde_json::from_str(&content).context("Failed to parse tasks.json")?;
        Ok(tasks)
    }

    /// Save all tasks
    pub fn save(&self, tasks: &[Task]) -> Result<()> {
        let content = serde_json::to_string_pretty(tasks).context("Failed to serialize tasks")?;
        fs::write(self.tasks_file(), content).context("Failed to write tasks.json")?;
        Ok(())
    }

    /// Add a task
    pub fn add(&self, task: Task) -> Result<()> {
        let mut tasks = self.load()?;
        tasks.push(task);
        self.save(&tasks)
    }

    /// Update a task
    #[allow(dead_code)]
    pub fn update(&self, task: &Task) -> Result<()> {
        let mut tasks = self.load()?;
        if let Some(existing) = tasks.iter_mut().find(|t| t.id == task.id) {
            *existing = task.clone();
            self.save(&tasks)?;
        }
        Ok(())
    }

    /// Delete a task
    pub fn delete(&self, task_id: &str) -> Result<()> {
        let mut tasks = self.load()?;
        tasks.retain(|t| t.id != task_id);
        self.save(&tasks)
    }

    /// Get task by ID
    #[allow(dead_code)]
    pub fn get(&self, task_id: &str) -> Result<Option<Task>> {
        let tasks = self.load()?;
        Ok(tasks.into_iter().find(|t| t.id == task_id))
    }

    /// Get tasks filtered by status
    #[allow(dead_code)]
    pub fn get_by_status(&self, status: TaskStatus) -> Result<Vec<Task>> {
        let tasks = self.load()?;
        Ok(tasks.into_iter().filter(|t| t.status == status).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_store_crud() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        // Create
        let task = Task::new("Test task", "Description");
        let task_id = task.id.clone();
        store.add(task).unwrap();

        // Read
        let loaded = store.get(&task_id).unwrap().unwrap();
        assert_eq!(loaded.title, "Test task");

        // Update
        let mut updated = loaded;
        updated.title = "Updated".into();
        store.update(&updated).unwrap();

        let reloaded = store.get(&task_id).unwrap().unwrap();
        assert_eq!(reloaded.title, "Updated");

        // Delete
        store.delete(&task_id).unwrap();
        assert!(store.get(&task_id).unwrap().is_none());
    }

    #[test]
    fn test_store_persists_planner_executor() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        let mut task = Task::new("Feature", "Implement something");
        task.assign_planner("gemini");
        task.assign_executor("claude", "hive/feature");
        task.set_status(TaskStatus::InProgress);
        let task_id = task.id.clone();

        store.add(task).unwrap();

        // Reload and verify planner/executor are persisted
        let loaded = store.get(&task_id).unwrap().unwrap();
        assert_eq!(loaded.planner, Some("gemini".into()));
        assert_eq!(loaded.executor, Some("claude".into()));
        assert_eq!(loaded.agent, Some("claude".into())); // backward compat
        assert_eq!(loaded.branch, Some("hive/feature".into()));
        assert_eq!(loaded.status, TaskStatus::InProgress);
    }

    #[test]
    fn test_store_persists_all_statuses() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        // Create tasks with different statuses
        let statuses = [
            TaskStatus::Todo,
            TaskStatus::Planning,
            TaskStatus::PlanReview,
            TaskStatus::InProgress,
            TaskStatus::Review,
            TaskStatus::Done,
            TaskStatus::Cancelled,
        ];

        for (i, status) in statuses.iter().enumerate() {
            let mut task = Task::new(format!("Task {}", i), "");
            task.set_status(*status);
            store.add(task).unwrap();
        }

        // Verify all are loaded with correct statuses
        let tasks = store.load().unwrap();
        assert_eq!(tasks.len(), 7);

        for status in statuses {
            let found = tasks.iter().any(|t| t.status == status);
            assert!(found, "Status {:?} should be found", status);
        }
    }

    #[test]
    fn test_get_by_status() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        // Add tasks with different statuses
        let task1 = Task::new("Todo 1", "");
        let task2 = Task::new("Todo 2", "");
        let mut task3 = Task::new("In Progress", "");
        task3.set_status(TaskStatus::InProgress);

        store.add(task1).unwrap();
        store.add(task2).unwrap();
        store.add(task3).unwrap();

        let todo_tasks = store.get_by_status(TaskStatus::Todo).unwrap();
        assert_eq!(todo_tasks.len(), 2);

        let in_progress_tasks = store.get_by_status(TaskStatus::InProgress).unwrap();
        assert_eq!(in_progress_tasks.len(), 1);

        let done_tasks = store.get_by_status(TaskStatus::Done).unwrap();
        assert_eq!(done_tasks.len(), 0);
    }

    #[test]
    fn test_store_empty_initially() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        let tasks = store.load().unwrap();
        assert!(tasks.is_empty());
    }

    #[test]
    fn test_store_creates_hive_dir() {
        let dir = tempdir().unwrap();
        let _store = TaskStore::new(dir.path()).unwrap();

        let hive_dir = dir.path().join(".hive");
        assert!(hive_dir.exists());
    }

    #[test]
    fn test_store_persists_worktree() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        let mut task = Task::new("Task", "");
        task.worktree = Some("/path/to/worktree".into());
        task.output_log = Some("/path/to/log".into());
        let task_id = task.id.clone();

        store.add(task).unwrap();

        let loaded = store.get(&task_id).unwrap().unwrap();
        assert_eq!(loaded.worktree, Some("/path/to/worktree".into()));
        assert_eq!(loaded.output_log, Some("/path/to/log".into()));
    }

    // ========================================
    // Error Handling Tests
    // ========================================

    #[test]
    fn test_load_handles_corrupted_json() {
        let dir = tempdir().unwrap();
        let hive_dir = dir.path().join(".hive");
        std::fs::create_dir_all(&hive_dir).unwrap();

        let tasks_file = hive_dir.join("tasks.json");
        std::fs::write(&tasks_file, "{ invalid json }").unwrap();

        let store = TaskStore::new(dir.path()).unwrap();
        let result = store.load();

        // Should return error for invalid JSON
        assert!(result.is_err());
    }

    #[test]
    fn test_update_nonexistent_task() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        let task = Task::new("Nonexistent", "");
        // Update should succeed even if task doesn't exist (it just won't change anything)
        let result = store.update(&task);
        assert!(result.is_ok());
    }

    #[test]
    fn test_delete_nonexistent_task() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        // Delete should succeed even if task doesn't exist
        let result = store.delete("nonexistent-id");
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_nonexistent_task() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        let result = store.get("nonexistent-id").unwrap();
        assert!(result.is_none());
    }

    // ========================================
    // Multiple Tasks Tests
    // ========================================

    #[test]
    fn test_store_multiple_tasks() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        for i in 0..10 {
            let task = Task::new(format!("Task {}", i), format!("Description {}", i));
            store.add(task).unwrap();
        }

        let tasks = store.load().unwrap();
        assert_eq!(tasks.len(), 10);
    }

    #[test]
    fn test_save_preserves_order() {
        let dir = tempdir().unwrap();
        let store = TaskStore::new(dir.path()).unwrap();

        let task1 = Task::new("First", "");
        let task2 = Task::new("Second", "");
        let task3 = Task::new("Third", "");

        let id1 = task1.id.clone();
        let id2 = task2.id.clone();
        let id3 = task3.id.clone();

        store.add(task1).unwrap();
        store.add(task2).unwrap();
        store.add(task3).unwrap();

        let tasks = store.load().unwrap();
        assert_eq!(tasks[0].id, id1);
        assert_eq!(tasks[1].id, id2);
        assert_eq!(tasks[2].id, id3);
    }
}
