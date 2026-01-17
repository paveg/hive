use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Git worktree manager
pub struct WorktreeManager {
    /// Repository root path
    repo_root: PathBuf,
    /// Directory for creating worktrees
    worktree_dir: PathBuf,
    /// Branch name prefix
    branch_prefix: String,
}

impl WorktreeManager {
    pub fn new(repo_root: PathBuf, hive_dir: PathBuf) -> Self {
        let worktree_dir = hive_dir.join("worktrees");
        std::fs::create_dir_all(&worktree_dir).ok();

        Self {
            repo_root,
            worktree_dir,
            branch_prefix: "hive".into(),
        }
    }

    /// Create worktree for a task
    pub fn create(&self, task_id: &str) -> Result<PathBuf> {
        let branch_name = format!("{}/{}", self.branch_prefix, task_id);
        let worktree_path = self.worktree_dir.join(task_id);

        // Return existing path if already exists
        if worktree_path.exists() {
            return Ok(worktree_path);
        }

        // Create worktree with a new branch
        let output = Command::new("git")
            .args(["worktree", "add", "-b", &branch_name])
            .arg(&worktree_path)
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to execute git worktree add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If branch already exists, try to create with existing branch
            if stderr.contains("already exists") {
                let output = Command::new("git")
                    .args(["worktree", "add"])
                    .arg(&worktree_path)
                    .arg(&branch_name)
                    .current_dir(&self.repo_root)
                    .output()
                    .context("Failed to execute git worktree add with existing branch")?;

                if !output.status.success() {
                    bail!(
                        "Failed to create worktree: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            } else {
                bail!("Failed to create worktree: {}", stderr);
            }
        }

        // Set up .claude/settings.json (plansDirectory)
        self.setup_claude_settings(&worktree_path)?;

        Ok(worktree_path)
    }

    /// Remove worktree
    pub fn remove(&self, task_id: &str) -> Result<()> {
        let worktree_path = self.worktree_dir.join(task_id);

        if !worktree_path.exists() {
            return Ok(());
        }

        // Remove worktree
        let output = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_path)
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to execute git worktree remove")?;

        if !output.status.success() {
            bail!(
                "Failed to remove worktree: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    /// Get worktree path
    #[allow(dead_code)]
    pub fn get_path(&self, task_id: &str) -> PathBuf {
        self.worktree_dir.join(task_id)
    }

    /// Check if worktree exists
    pub fn exists(&self, task_id: &str) -> bool {
        self.worktree_dir.join(task_id).exists()
    }

    /// Get branch name
    pub fn get_branch_name(&self, task_id: &str) -> String {
        format!("{}/{}", self.branch_prefix, task_id)
    }

    /// Create Claude Code settings file
    fn setup_claude_settings(&self, worktree_path: &PathBuf) -> Result<()> {
        let claude_dir = worktree_path.join(".claude");
        std::fs::create_dir_all(&claude_dir).context("Failed to create .claude directory")?;

        // Point plansDirectory to .hive/plans (relative path)
        let settings = serde_json::json!({
            "plansDirectory": "../../plans"
        });

        let settings_path = claude_dir.join("settings.json");
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&settings).unwrap(),
        )
        .context("Failed to write .claude/settings.json")?;

        Ok(())
    }

    /// Get diff from main branch
    pub fn get_diff(&self, task_id: &str, base_branch: &str) -> Result<String> {
        let worktree_path = self.worktree_dir.join(task_id);

        let output = Command::new("git")
            .args(["diff", base_branch])
            .current_dir(&worktree_path)
            .output()
            .context("Failed to execute git diff")?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Merge changes
    pub fn merge(&self, task_id: &str, _target_branch: &str) -> Result<()> {
        let branch_name = self.get_branch_name(task_id);

        // Merge in main repository
        let output = Command::new("git")
            .args(["merge", &branch_name, "--no-ff", "-m"])
            .arg(format!("Merge {} via Hive", task_id))
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to execute git merge")?;

        if !output.status.success() {
            bail!(
                "Failed to merge: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_manager() -> (TempDir, WorktreeManager) {
        let temp_dir = TempDir::new().unwrap();
        let hive_dir = temp_dir.path().join(".hive");
        let manager = WorktreeManager::new(temp_dir.path().to_path_buf(), hive_dir);
        (temp_dir, manager)
    }

    // ========================================
    // Branch Name Generation Tests
    // ========================================

    #[test]
    fn test_get_branch_name() {
        let (_temp, manager) = create_test_manager();

        let branch = manager.get_branch_name("task-abc123");
        assert_eq!(branch, "hive/task-abc123");
    }

    #[test]
    fn test_get_branch_name_various_ids() {
        let (_temp, manager) = create_test_manager();

        assert_eq!(manager.get_branch_name("task-1"), "hive/task-1");
        assert_eq!(manager.get_branch_name("feature-xyz"), "hive/feature-xyz");
        assert_eq!(manager.get_branch_name("123"), "hive/123");
    }

    // ========================================
    // Path Generation Tests
    // ========================================

    #[test]
    fn test_get_path() {
        let (temp_dir, manager) = create_test_manager();

        let path = manager.get_path("task-abc123");
        let expected = temp_dir.path().join(".hive/worktrees/task-abc123");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_worktrees_dir_created() {
        let (temp_dir, _manager) = create_test_manager();

        let worktrees_dir = temp_dir.path().join(".hive/worktrees");
        assert!(worktrees_dir.exists());
    }

    // ========================================
    // Exists Check Tests
    // ========================================

    #[test]
    fn test_exists_false_when_not_created() {
        let (_temp, manager) = create_test_manager();

        assert!(!manager.exists("nonexistent-task"));
    }

    // ========================================
    // Edge Cases
    // ========================================

    #[test]
    fn test_special_characters_in_task_id() {
        let (_temp, manager) = create_test_manager();

        // These should work as branch names
        let branch = manager.get_branch_name("task-with-dashes");
        assert_eq!(branch, "hive/task-with-dashes");

        let branch = manager.get_branch_name("task_with_underscores");
        assert_eq!(branch, "hive/task_with_underscores");
    }
}
