use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

/// Git repository validation result
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether validation passed
    pub is_valid: bool,
    /// Warning messages (operation possible but caution needed)
    pub warnings: Vec<String>,
    /// Error messages (should block operation)
    pub errors: Vec<String>,
}

impl ValidationResult {
    pub fn ok() -> Self {
        Self {
            is_valid: true,
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn with_warning(mut self, msg: impl Into<String>) -> Self {
        self.warnings.push(msg.into());
        self
    }

    pub fn with_error(mut self, msg: impl Into<String>) -> Self {
        self.is_valid = false;
        self.errors.push(msg.into());
        self
    }

    #[allow(dead_code)]
    pub fn merge(mut self, other: ValidationResult) -> Self {
        self.is_valid = self.is_valid && other.is_valid;
        self.warnings.extend(other.warnings);
        self.errors.extend(other.errors);
        self
    }
}

/// Git repository validator
pub struct GitValidator {
    repo_root: PathBuf,
}

impl GitValidator {
    pub fn new(repo_root: PathBuf) -> Self {
        Self { repo_root }
    }

    /// Check if this is a git repository
    pub fn is_git_repo(&self) -> bool {
        let output = Command::new("git")
            .args(["rev-parse", "--git-dir"])
            .current_dir(&self.repo_root)
            .output();

        matches!(output, Ok(o) if o.status.success())
    }

    /// Check if main repository has uncommitted changes
    pub fn has_uncommitted_changes(&self) -> Result<bool> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to execute git status")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
    }

    /// Check if main repository has staged changes
    pub fn has_staged_changes(&self) -> Result<bool> {
        let output = Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to execute git diff --cached")?;

        // --quiet returns non-zero if there are differences
        Ok(!output.status.success())
    }

    /// Get current branch name
    #[allow(dead_code)]
    pub fn current_branch(&self) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to get current branch")?;

        if !output.status.success() {
            bail!("Failed to get current branch");
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Check if branch exists
    pub fn branch_exists(&self, branch_name: &str) -> Result<bool> {
        let output = Command::new("git")
            .args(["rev-parse", "--verify", branch_name])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to check branch existence")?;

        Ok(output.status.success())
    }

    /// Get list of registered worktrees
    #[allow(dead_code)]
    pub fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>> {
        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(&self.repo_root)
            .output()
            .context("Failed to list worktrees")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut worktrees = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        let mut current_branch: Option<String> = None;

        for line in stdout.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                // Save previous worktree
                if let Some(path) = current_path.take() {
                    worktrees.push(WorktreeInfo {
                        path,
                        branch: current_branch.take(),
                    });
                }
                current_path = Some(PathBuf::from(path));
            } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
                current_branch = Some(branch.to_string());
            }
        }

        // Save the last worktree
        if let Some(path) = current_path {
            worktrees.push(WorktreeInfo {
                path,
                branch: current_branch,
            });
        }

        Ok(worktrees)
    }

    /// Validate before creating worktree
    pub fn validate_for_worktree_creation(&self) -> Result<ValidationResult> {
        let mut result = ValidationResult::ok();

        // Check if git repository
        if !self.is_git_repo() {
            return Ok(result.with_error("Not a git repository"));
        }

        // Warn about uncommitted changes
        if self.has_uncommitted_changes()? {
            result = result.with_warning(
                "Main repository has uncommitted changes. They will not be reflected in the worktree.",
            );
        }

        // Warn about staged changes
        if self.has_staged_changes()? {
            result = result.with_warning(
                "There are staged changes. Committing first is recommended.",
            );
        }

        Ok(result)
    }

    /// Comprehensive validation before starting a task
    pub fn validate_for_task_start(&self, task_id: &str, branch_prefix: &str) -> Result<ValidationResult> {
        let mut result = self.validate_for_worktree_creation()?;

        // Check if branch already exists
        let branch_name = format!("{}/{}", branch_prefix, task_id);
        if self.branch_exists(&branch_name)? {
            result = result.with_warning(format!(
                "Branch '{}' already exists. Using the existing branch.",
                branch_name
            ));
        }

        Ok(result)
    }
}

/// Worktree information
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// Worktree artifact validator
pub struct WorktreeValidator {
    worktree_path: PathBuf,
}

impl WorktreeValidator {
    pub fn new(worktree_path: PathBuf) -> Self {
        Self { worktree_path }
    }

    /// Check if there are uncommitted changes
    pub fn has_changes(&self) -> Result<bool> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.worktree_path)
            .output()
            .context("Failed to execute git status")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(!stdout.trim().is_empty())
    }

    /// Check if there are new commits (compared to base_branch)
    pub fn has_new_commits(&self, base_branch: &str) -> Result<bool> {
        let output = Command::new("git")
            .args(["rev-list", "--count", &format!("{}..HEAD", base_branch)])
            .current_dir(&self.worktree_path)
            .output()
            .context("Failed to execute git rev-list")?;

        let count: i32 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0);

        Ok(count > 0)
    }

    /// Get count of changed files
    pub fn changed_file_count(&self, base_branch: &str) -> Result<usize> {
        let output = Command::new("git")
            .args(["diff", "--name-only", base_branch])
            .current_dir(&self.worktree_path)
            .output()
            .context("Failed to execute git diff")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let count = stdout.lines().filter(|l| !l.is_empty()).count();
        Ok(count)
    }

    /// Validate implementation completion (has changes or commits)
    pub fn validate_implementation(&self, base_branch: &str) -> Result<ValidationResult> {
        let mut result = ValidationResult::ok();

        // Check for new commits
        let has_commits = self.has_new_commits(base_branch)?;

        // Check for uncommitted changes
        let has_uncommitted = self.has_changes()?;

        if !has_commits && !has_uncommitted {
            return Ok(result.with_error("No changes found. Implementation may not be complete."));
        }

        if has_uncommitted {
            result = result.with_warning("There are uncommitted changes.");
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_git_repo() -> TempDir {
        let temp_dir = TempDir::new().unwrap();

        // Initialize git repo
        Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        // Configure git user for commits
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        // Create initial commit
        std::fs::write(temp_dir.path().join("README.md"), "# Test").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        temp_dir
    }

    // ========================================
    // ValidationResult Tests
    // ========================================

    #[test]
    fn test_validation_result_ok() {
        let result = ValidationResult::ok();
        assert!(result.is_valid);
        assert!(result.warnings.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validation_result_with_warning() {
        let result = ValidationResult::ok().with_warning("Some warning");
        assert!(result.is_valid); // Warning doesn't change is_valid
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.warnings[0], "Some warning");
    }

    #[test]
    fn test_validation_result_with_error() {
        let result = ValidationResult::ok().with_error("Some error");
        assert!(!result.is_valid); // Error sets is_valid to false
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn test_validation_result_merge() {
        let result1 = ValidationResult::ok().with_warning("Warning 1");
        let result2 = ValidationResult::ok().with_warning("Warning 2");
        let merged = result1.merge(result2);

        assert!(merged.is_valid);
        assert_eq!(merged.warnings.len(), 2);
    }

    #[test]
    fn test_validation_result_merge_with_error() {
        let result1 = ValidationResult::ok();
        let result2 = ValidationResult::ok().with_error("Error");
        let merged = result1.merge(result2);

        assert!(!merged.is_valid);
    }

    // ========================================
    // GitValidator Tests
    // ========================================

    #[test]
    fn test_is_git_repo_true() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        assert!(validator.is_git_repo());
    }

    #[test]
    fn test_is_git_repo_false() {
        let temp_dir = TempDir::new().unwrap();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        assert!(!validator.is_git_repo());
    }

    #[test]
    fn test_has_uncommitted_changes_clean() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        assert!(!validator.has_uncommitted_changes().unwrap());
    }

    #[test]
    fn test_has_uncommitted_changes_dirty() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        // Create an uncommitted file
        std::fs::write(temp_dir.path().join("new_file.txt"), "content").unwrap();

        assert!(validator.has_uncommitted_changes().unwrap());
    }

    #[test]
    fn test_has_staged_changes_none() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        assert!(!validator.has_staged_changes().unwrap());
    }

    #[test]
    fn test_has_staged_changes_some() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        // Create and stage a file
        std::fs::write(temp_dir.path().join("staged.txt"), "content").unwrap();
        Command::new("git")
            .args(["add", "staged.txt"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        assert!(validator.has_staged_changes().unwrap());
    }

    #[test]
    fn test_current_branch() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        // Default branch is usually "main" or "master"
        let branch = validator.current_branch().unwrap();
        assert!(!branch.is_empty());
    }

    #[test]
    fn test_branch_exists_true() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        // Get current branch and check it exists
        let current = validator.current_branch().unwrap();
        assert!(validator.branch_exists(&current).unwrap());
    }

    #[test]
    fn test_branch_exists_false() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        assert!(!validator.branch_exists("nonexistent-branch").unwrap());
    }

    #[test]
    fn test_list_worktrees() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        let worktrees = validator.list_worktrees().unwrap();

        // At minimum, main worktree should exist
        assert!(!worktrees.is_empty());
    }

    #[test]
    fn test_validate_for_worktree_creation_clean() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        let result = validator.validate_for_worktree_creation().unwrap();
        assert!(result.is_valid);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_validate_for_worktree_creation_with_uncommitted() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        // Create uncommitted change
        std::fs::write(temp_dir.path().join("dirty.txt"), "content").unwrap();

        let result = validator.validate_for_worktree_creation().unwrap();
        assert!(result.is_valid); // Only warnings, so is_valid is still true
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("uncommitted"));
    }

    #[test]
    fn test_validate_for_worktree_creation_not_git_repo() {
        let temp_dir = TempDir::new().unwrap();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        let result = validator.validate_for_worktree_creation().unwrap();
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_validate_for_task_start() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        let result = validator
            .validate_for_task_start("task-123", "hive")
            .unwrap();
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_for_task_start_existing_branch() {
        let temp_dir = create_git_repo();
        let validator = GitValidator::new(temp_dir.path().to_path_buf());

        // Create the branch first
        Command::new("git")
            .args(["branch", "hive/task-existing"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();

        let result = validator
            .validate_for_task_start("task-existing", "hive")
            .unwrap();
        assert!(result.is_valid);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("already exists"));
    }
}
