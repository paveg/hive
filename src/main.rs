mod agent;
mod git;
mod task;

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};
use tokio::sync::mpsc;

use agent::{AgentRunner, AgentStatus, OrchestratorConfig, PlanManager};
use git::{GitValidator, WorktreeManager, WorktreeValidator};
use task::{Task, TaskStatus, TaskStore};

/// Events from agents
#[derive(Debug, Clone)]
enum AgentEvent {
    /// Task completed
    Completed { task_id: String },
    /// Task failed
    Failed { task_id: String, error: String },
    /// Output line
    Output { task_id: String, line: String },
}

/// Input mode
#[derive(Debug, Clone, PartialEq)]
enum InputMode {
    Normal,
    /// Creating task (entering title)
    NewTaskTitle,
    /// Creating task (entering description)
    NewTaskDescription,
    /// Selecting planner
    SelectPlanner,
    /// Selecting executor
    SelectExecutor,
    /// Viewing task details
    TaskDetail,
    /// Viewing diff
    ViewDiff,
    /// Confirming merge
    ConfirmMerge,
}

/// Log entry for agent output
struct LogEntry {
    task_title: String,
    line: String,
}

/// Application state
struct App {
    /// Task store
    store: TaskStore,
    /// All tasks
    tasks: Vec<Task>,
    /// Currently selected column
    selected_column: usize,
    /// Selected task index for each column
    selected_task: [usize; 4],
    /// Input mode
    input_mode: InputMode,
    /// Input buffer
    input_buffer: String,
    /// Pending task title (temporary storage)
    pending_title: String,
    /// Status message
    status_message: Option<String>,
    /// Worktree manager
    worktree_manager: WorktreeManager,
    /// Git validator
    git_validator: GitValidator,
    /// Orchestrator config
    orchestrator: OrchestratorConfig,
    /// Plan manager
    plan_manager: PlanManager,
    /// Selection list (shared for Planner/Executor)
    selection_list: Vec<String>,
    /// Selected index
    selected_index: usize,
    /// Agent runner (shared)
    agent_runner: Arc<Mutex<AgentRunner>>,
    /// Agent event receiver
    agent_event_rx: mpsc::Receiver<AgentEvent>,
    /// Agent event sender (for cloning)
    agent_event_tx: mpsc::Sender<AgentEvent>,
    /// Diff content (for ViewDiff mode)
    diff_content: String,
    /// Scroll offset for diff view
    diff_scroll: usize,
    /// Running agent count (cached)
    running_count: usize,
    /// Agent log buffer (recent output lines)
    agent_logs: std::collections::VecDeque<LogEntry>,
}

impl App {
    fn new() -> anyhow::Result<Self> {
        let repo_root = PathBuf::from(".");
        let hive_dir = PathBuf::from(".hive");
        let store = TaskStore::new(&repo_root)?;
        let tasks = store.load()?;
        let worktree_manager = WorktreeManager::new(repo_root.clone(), hive_dir.clone());
        let git_validator = GitValidator::new(repo_root);
        let orchestrator = OrchestratorConfig::load(&hive_dir).unwrap_or_default();
        let plan_manager = PlanManager::new(hive_dir.clone());
        let agent_runner = Arc::new(Mutex::new(AgentRunner::new(hive_dir)));
        let (agent_event_tx, agent_event_rx) = mpsc::channel(100);

        Ok(Self {
            store,
            tasks,
            selected_column: 0,
            selected_task: [0; 4],
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            pending_title: String::new(),
            status_message: None,
            worktree_manager,
            git_validator,
            orchestrator,
            plan_manager,
            selection_list: vec![],
            selected_index: 0,
            agent_runner,
            agent_event_rx,
            agent_event_tx,
            diff_content: String::new(),
            diff_scroll: 0,
            running_count: 0,
            agent_logs: std::collections::VecDeque::with_capacity(100),
        })
    }

    /// Get tasks in specified column
    fn tasks_in_column(&self, column: usize) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.status.to_column_index() == Some(column))
            .collect()
    }

    /// Get currently selected task
    fn selected_task(&self) -> Option<&Task> {
        let tasks = self.tasks_in_column(self.selected_column);
        tasks.get(self.selected_task[self.selected_column]).copied()
    }

    /// Get currently selected task (mutable)
    fn selected_task_mut(&mut self) -> Option<&mut Task> {
        let column = self.selected_column;
        let idx = self.selected_task[column];
        self.tasks
            .iter_mut()
            .filter(|t| t.status.to_column_index() == Some(column))
            .nth(idx)
    }

    fn move_left(&mut self) {
        if self.selected_column > 0 {
            self.selected_column -= 1;
            self.clamp_selection();
        }
    }

    fn move_right(&mut self) {
        if self.selected_column < 3 {
            self.selected_column += 1;
            self.clamp_selection();
        }
    }

    fn move_up(&mut self) {
        let col = self.selected_column;
        if self.selected_task[col] > 0 {
            self.selected_task[col] -= 1;
        }
    }

    fn move_down(&mut self) {
        let col = self.selected_column;
        let count = self.tasks_in_column(col).len();
        if count > 0 && self.selected_task[col] < count - 1 {
            self.selected_task[col] += 1;
        }
    }

    /// Clamp selection index within range
    fn clamp_selection(&mut self) {
        let col = self.selected_column;
        let count = self.tasks_in_column(col).len();
        if count == 0 {
            self.selected_task[col] = 0;
        } else if self.selected_task[col] >= count {
            self.selected_task[col] = count - 1;
        }
    }

    /// Start new task creation
    fn start_new_task(&mut self) {
        self.input_mode = InputMode::NewTaskTitle;
        self.input_buffer.clear();
        self.status_message = Some("Enter task title (ESC to cancel)".into());
    }

    /// Start agent selection (select Planner/Executor based on status)
    fn start_assign_agent(&mut self) {
        if let Some(task) = self.selected_task() {
            match task.status {
                TaskStatus::Todo => {
                    // Planner選択
                    self.selection_list = self.orchestrator.available_planners()
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect();
                    self.selected_index = 0;
                    self.input_mode = InputMode::SelectPlanner;
                    self.status_message = Some("Select Planner (Enter to confirm, ESC to cancel)".into());
                }
                TaskStatus::PlanReview => {
                    // Executor選択
                    self.selection_list = self.orchestrator.available_executors()
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect();
                    self.selected_index = 0;
                    self.input_mode = InputMode::SelectExecutor;
                    self.status_message = Some("Select Executor (Enter to confirm, ESC to cancel)".into());
                }
                _ => {
                    self.status_message = Some("Cannot assign agent in this status".into());
                }
            }
        } else {
            self.status_message = Some("No task selected".into());
        }
    }

    /// Assign planner and start workflow
    fn assign_planner(&mut self) -> anyhow::Result<()> {
        let planner_name = self.selection_list[self.selected_index].clone();
        let task_id = self
            .selected_task()
            .ok_or_else(|| anyhow::anyhow!("No task selected"))?
            .id
            .clone();
        self.input_mode = InputMode::Normal;
        self.start_planner_for_task(&task_id, &planner_name)
    }

    /// Start planning for a specific task with the given planner
    fn start_planner_for_task(
        &mut self,
        task_id: &str,
        planner_name: &str,
    ) -> anyhow::Result<()> {
        // Get task info
        let (task_title, task_description) = {
            let task = self
                .tasks
                .iter()
                .find(|t| t.id == task_id)
                .ok_or_else(|| anyhow::anyhow!("Task not found"))?;
            (task.title.clone(), task.description.clone())
        };

        // Run git validation
        let validation = self.git_validator.validate_for_task_start(task_id, "hive")?;
        if !validation.is_valid {
            self.status_message = Some(format!("❌ {}", validation.errors.join(", ")));
            return Ok(());
        }

        // Create worktree
        let worktree_path = self.worktree_manager.create(task_id)?;
        let branch_name = self.worktree_manager.get_branch_name(task_id);

        // Update task
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            task.assign_planner(planner_name);
            task.branch = Some(branch_name.clone());
            task.worktree = Some(worktree_path.to_string_lossy().to_string());
            task.set_status(TaskStatus::Planning);
        }

        self.store.save(&self.tasks)?;

        // Create planning prompt with plan file path
        let prompt = self
            .plan_manager
            .create_planning_prompt(task_id, &task_title, &task_description);

        // Start agent in background
        self.start_agent(
            task_id.to_string(),
            planner_name,
            worktree_path,
            prompt,
        );

        self.status_message = Some(format!(
            "🧠 Planner '{}' started for '{}' (branch: {})",
            planner_name, task_title, branch_name
        ));

        Ok(())
    }

    /// Assign executor and start implementation
    fn assign_executor(&mut self) -> anyhow::Result<()> {
        let executor_name = self.selection_list[self.selected_index].clone();
        let task_id = self
            .selected_task()
            .ok_or_else(|| anyhow::anyhow!("No task selected"))?
            .id
            .clone();
        self.input_mode = InputMode::Normal;
        self.start_executor_for_task(&task_id, &executor_name)
    }

    /// Start execution for a specific task with the given executor
    fn start_executor_for_task(
        &mut self,
        task_id: &str,
        executor_name: &str,
    ) -> anyhow::Result<()> {
        // Get task info
        let (task_title, worktree_path, branch) = {
            let task = self
                .tasks
                .iter()
                .find(|t| t.id == task_id)
                .ok_or_else(|| anyhow::anyhow!("Task not found"))?;
            let worktree = task
                .worktree
                .clone()
                .ok_or_else(|| anyhow::anyhow!("No worktree"))?;
            (
                task.title.clone(),
                PathBuf::from(worktree),
                task.branch.clone().unwrap_or_default(),
            )
        };

        // Create execution prompt
        let prompt = self.plan_manager.create_execution_prompt(task_id)?;

        // Update task (worktree already created during Planner phase)
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            task.assign_executor(executor_name, &branch);
            task.set_status(TaskStatus::InProgress);
        }

        self.store.save(&self.tasks)?;

        // Start agent in background
        self.start_agent(
            task_id.to_string(),
            executor_name,
            worktree_path,
            prompt,
        );

        self.status_message = Some(format!(
            "🔨 Executor '{}' started for '{}'",
            executor_name, task_title
        ));

        Ok(())
    }

    /// Move task to next status (with strict validation)
    fn move_task_forward(&mut self) -> anyhow::Result<()> {
        // First validate in read-only mode
        let advance_result = {
            let task = match self.selected_task() {
                Some(t) => t,
                None => return Ok(()),
            };

            match task.can_advance() {
                Ok(new_status) => {
                    // For Planning → PlanReview, check if plan file exists
                    if task.status == TaskStatus::Planning {
                        if !self.plan_manager.plan_file_exists(&task.id) {
                            return Ok(self.status_message = Some("Plan has not been created".into()));
                        }
                    }
                    Ok(new_status)
                }
                Err(msg) => Err(msg),
            }
        };

        // Update if validation succeeded
        match advance_result {
            Ok(new_status) => {
                if let Some(task) = self.selected_task_mut() {
                    task.set_status(new_status);
                    self.store.save(&self.tasks)?;
                    self.status_message = Some(format!("Moved to {}", new_status.display_name()));
                    self.clamp_selection();
                }
            }
            Err(msg) => {
                self.status_message = Some(msg.to_string());
            }
        }
        Ok(())
    }

    /// Move task to previous status (for plan revision)
    fn move_task_backward(&mut self) -> anyhow::Result<()> {
        if let Some(task) = self.selected_task_mut() {
            if let Some(new_status) = task.retreat_target() {
                task.set_status(new_status);
                self.store.save(&self.tasks)?;
                self.status_message = Some(format!("Moved back to {}", new_status.display_name()));
                self.clamp_selection();
            }
        }
        Ok(())
    }

    /// Delete task (using TaskStore)
    fn delete_task(&mut self) -> anyhow::Result<()> {
        if let Some(task) = self.selected_task() {
            let id = task.id.clone();
            // Remove worktree if exists
            if task.worktree.is_some() {
                let _ = self.worktree_manager.remove(&id);
            }
            self.store.delete(&id)?;
            self.tasks = self.store.load()?;
            self.status_message = Some("Task deleted".into());
            self.clamp_selection();
        }
        Ok(())
    }

    /// Show task detail view
    fn show_task_detail(&mut self) {
        if self.selected_task().is_some() {
            self.input_mode = InputMode::TaskDetail;
            self.status_message = Some("Task Detail (ESC to close, s to stop agent, d for diff)".into());
        } else {
            self.status_message = Some("No task selected".into());
        }
    }

    /// Stop running agent for selected task
    fn stop_agent(&mut self) {
        let task_id = match self.selected_task() {
            Some(t) => t.id.clone(),
            None => return,
        };

        let runner = Arc::clone(&self.agent_runner);
        let event_tx = self.agent_event_tx.clone();

        tokio::spawn(async move {
            let mut runner = runner.lock().await;
            if let Err(e) = runner.stop(&task_id).await {
                let _ = event_tx
                    .send(AgentEvent::Failed {
                        task_id,
                        error: format!("Failed to stop: {}", e),
                    })
                    .await;
            }
        });

        self.status_message = Some("Stopping agent...".into());
    }

    /// Show diff view for selected task
    fn show_diff(&mut self) -> anyhow::Result<()> {
        if let Some(task) = self.selected_task() {
            if task.worktree.is_some() {
                // Check if worktree exists
                if self.worktree_manager.exists(&task.id) {
                    let diff = self.worktree_manager.get_diff(&task.id, "main")?;
                    if diff.is_empty() {
                        self.status_message = Some("No changes found".into());
                    } else {
                        self.diff_content = diff;
                        self.diff_scroll = 0;
                        self.input_mode = InputMode::ViewDiff;
                        self.status_message = Some("Diff View (j/k scroll, ESC close)".into());
                    }
                } else {
                    self.status_message = Some("Worktree not found".into());
                }
            } else {
                self.status_message = Some("No worktree for this task".into());
            }
        }
        Ok(())
    }

    /// Create PR for selected task
    fn create_pr(&mut self) -> anyhow::Result<()> {
        let task = match self.selected_task() {
            Some(t) => t,
            None => {
                self.status_message = Some("No task selected".into());
                return Ok(());
            }
        };

        if task.status != TaskStatus::Review {
            self.status_message = Some("Task must be in Review status to create PR".into());
            return Ok(());
        }

        let branch = match &task.branch {
            Some(b) => b.clone(),
            None => {
                self.status_message = Some("No branch for this task".into());
                return Ok(());
            }
        };

        let worktree = match &task.worktree {
            Some(w) => w.clone(),
            None => {
                self.status_message = Some("No worktree for this task".into());
                return Ok(());
            }
        };

        let title = task.title.clone();
        let description = task.description.clone();

        // Create PR using gh command
        self.status_message = Some(format!("Creating PR for '{}'...", title));

        let pr_body = format!(
            "## Summary\n{}\n\n## Task\nCreated via Hive AI Agent Orchestration\n\n---\n🤖 Generated with Hive",
            if description.is_empty() { &title } else { &description }
        );

        let output = std::process::Command::new("gh")
            .args(["pr", "create", "--title", &title, "--body", &pr_body, "--head", &branch])
            .current_dir(&worktree)
            .output();

        match output {
            Ok(result) => {
                if result.status.success() {
                    let url = String::from_utf8_lossy(&result.stdout).trim().to_string();
                    self.status_message = Some(format!("✅ PR created: {}", url));
                } else {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    self.status_message = Some(format!("❌ PR failed: {}", stderr.trim()));
                }
            }
            Err(e) => {
                self.status_message = Some(format!("❌ Failed to run gh: {}", e));
            }
        }

        Ok(())
    }

    /// Start merge confirmation
    fn start_merge(&mut self) {
        if let Some(task) = self.selected_task() {
            if task.status == TaskStatus::Review {
                // Validate implementation before merge
                if let Some(ref worktree) = task.worktree {
                    let validator = WorktreeValidator::new(PathBuf::from(worktree));
                    let validation = validator.validate_implementation("main");

                    match validation {
                        Ok(result) => {
                            if !result.is_valid {
                                self.status_message = Some(format!("❌ {}", result.errors.join(", ")));
                                return;
                            }
                            if !result.warnings.is_empty() {
                                self.status_message = Some(format!("⚠️ {}", result.warnings.join(", ")));
                            }
                        }
                        Err(e) => {
                            self.status_message = Some(format!("Validation error: {}", e));
                            return;
                        }
                    }
                }

                self.input_mode = InputMode::ConfirmMerge;
                self.status_message = Some("Merge to main? (y/n)".into());
            } else {
                self.status_message = Some("Can only merge from Review status".into());
            }
        }
    }

    /// Execute merge
    fn execute_merge(&mut self) -> anyhow::Result<()> {
        if let Some(task) = self.selected_task() {
            let task_id = task.id.clone();
            let title = task.title.clone();

            // Get changed file count for summary
            let changed_files = if let Some(ref worktree) = task.worktree {
                let validator = WorktreeValidator::new(PathBuf::from(worktree));
                validator.changed_file_count("main").unwrap_or(0)
            } else {
                0
            };

            // Execute merge
            self.worktree_manager.merge(&task_id, "main")?;

            // Update task status
            if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                task.set_status(TaskStatus::Done);
            }
            self.store.save(&self.tasks)?;

            // Clean up worktree
            let _ = self.worktree_manager.remove(&task_id);

            self.input_mode = InputMode::Normal;
            self.status_message = Some(format!(
                "✅ Merged '{}' ({} files changed)",
                title, changed_files
            ));
            self.clamp_selection();
        }
        Ok(())
    }

    /// Handle input
    fn handle_input(&mut self, c: char) {
        self.input_buffer.push(c);
    }

    /// Handle backspace
    fn handle_backspace(&mut self) {
        self.input_buffer.pop();
    }

    /// Confirm input
    fn confirm_input(&mut self) -> anyhow::Result<()> {
        match self.input_mode {
            InputMode::NewTaskTitle => {
                if !self.input_buffer.is_empty() {
                    self.pending_title = self.input_buffer.clone();
                    self.input_buffer.clear();
                    self.input_mode = InputMode::NewTaskDescription;
                    self.status_message = Some("Enter description (Enter to skip)".into());
                }
            }
            InputMode::NewTaskDescription => {
                let task = Task::new(&self.pending_title, &self.input_buffer);
                let task_id = task.id.clone();
                self.store.add(task)?;
                self.tasks = self.store.load()?;
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                self.pending_title.clear();

                // Auto-start planning with default planner
                let default_planner = self.orchestrator.default_planner.clone();
                self.start_planner_for_task(&task_id, &default_planner)?;
            }
            InputMode::SelectPlanner => {
                self.assign_planner()?;
            }
            InputMode::SelectExecutor => {
                self.assign_executor()?;
            }
            InputMode::ConfirmMerge => {
                // Enter confirms merge
                self.execute_merge()?;
            }
            InputMode::Normal | InputMode::TaskDetail | InputMode::ViewDiff => {}
        }
        Ok(())
    }

    /// Cancel input / close popup
    fn cancel_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.pending_title.clear();
        self.diff_content.clear();
        self.diff_scroll = 0;
        self.status_message = None;
    }

    /// Scroll diff view
    fn scroll_diff(&mut self, direction: i32) {
        let lines = self.diff_content.lines().count();
        if direction > 0 && self.diff_scroll < lines.saturating_sub(20) {
            self.diff_scroll += 1;
        } else if direction < 0 && self.diff_scroll > 0 {
            self.diff_scroll -= 1;
        }
    }

    /// Move selection up
    fn selection_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    fn selection_down(&mut self) {
        if self.selected_index < self.selection_list.len().saturating_sub(1) {
            self.selected_index += 1;
        }
    }

    /// Start agent in background
    fn start_agent(
        &self,
        task_id: String,
        agent_name: &str,
        working_dir: PathBuf,
        prompt: String,
    ) {
        let agent_runner = Arc::clone(&self.agent_runner);
        let event_tx = self.agent_event_tx.clone();
        let agent_name = agent_name.to_string();

        tokio::spawn(async move {
            // Get AgentConfig
            let config = match agent::AgentConfig::from_name(&agent_name) {
                Some(c) => c,
                None => {
                    let _ = event_tx
                        .send(AgentEvent::Failed {
                            task_id: task_id.clone(),
                            error: format!("Unknown agent: {}", agent_name),
                        })
                        .await;
                    return;
                }
            };

            // Start agent
            let rx = {
                let mut runner = agent_runner.lock().await;
                match runner.start(&task_id, config, working_dir, &prompt).await {
                    Ok(rx) => rx,
                    Err(e) => {
                        let _ = event_tx
                            .send(AgentEvent::Failed {
                                task_id: task_id.clone(),
                                error: e.to_string(),
                            })
                            .await;
                        return;
                    }
                }
            };

            // Forward output
            let mut rx = rx;
            while let Some(line) = rx.recv().await {
                let _ = event_tx
                    .send(AgentEvent::Output {
                        task_id: task_id.clone(),
                        line,
                    })
                    .await;
            }

            // Check completion
            let status = {
                let mut runner = agent_runner.lock().await;
                runner.check_task_completion(&task_id)
            };

            if let Some(status) = status {
                match status {
                    AgentStatus::Completed => {
                        let _ = event_tx
                            .send(AgentEvent::Completed {
                                task_id: task_id.clone(),
                            })
                            .await;
                    }
                    AgentStatus::Failed(error) => {
                        let _ = event_tx
                            .send(AgentEvent::Failed {
                                task_id: task_id.clone(),
                                error,
                            })
                            .await;
                    }
                    _ => {}
                }
            }
        });
    }

    /// Process agent events (non-blocking)
    async fn process_agent_events(&mut self) -> anyhow::Result<()> {
        while let Ok(event) = self.agent_event_rx.try_recv() {
            match event {
                AgentEvent::Completed { task_id } => {
                    self.handle_agent_completed(&task_id)?;
                }
                AgentEvent::Failed { task_id, error } => {
                    // Collect task info and update in a scope to end mutable borrow
                    let task_info = if let Some(task) =
                        self.tasks.iter_mut().find(|t| t.id == task_id)
                    {
                        // Revert status and clear agent assignment on failure
                        let (new_status, cleared) = match task.status {
                            TaskStatus::Planning => {
                                task.planner = None;
                                (TaskStatus::Todo, "planner")
                            }
                            TaskStatus::InProgress => {
                                task.executor = None;
                                (TaskStatus::PlanReview, "executor")
                            }
                            _ => (task.status, ""),
                        };
                        task.set_status(new_status);
                        Some((task.title.clone(), new_status, cleared))
                    } else {
                        None
                    };

                    // Save and show message after mutable borrow ends
                    if let Some((title, new_status, cleared)) = task_info {
                        if let Err(e) = self.store.save(&self.tasks) {
                            self.status_message = Some(format!("❌ Save error: {}", e));
                        } else {
                            self.status_message = Some(format!(
                                "❌ Agent failed on '{}': {} (reverted to {}, {} cleared)",
                                title, error, new_status.display_name(), cleared
                            ));
                        }
                    }
                }
                AgentEvent::Output { task_id, line } => {
                    // Store output in log buffer
                    if let Some(task) = self.tasks.iter().find(|t| t.id == task_id) {
                        let task_title = task.title.clone();

                        // Add to log buffer (keep max 100 entries)
                        if self.agent_logs.len() >= 100 {
                            self.agent_logs.pop_front();
                        }
                        self.agent_logs.push_back(LogEntry {
                            task_title: task_title.clone(),
                            line: line.clone(),
                        });

                        // Also update status message with truncated line
                        let truncated = if line.chars().count() > 60 {
                            format!("{}...", line.chars().take(57).collect::<String>())
                        } else {
                            line
                        };
                        self.status_message = Some(format!("📝 {}: {}", task_title, truncated));
                    }
                }
            }
        }
        Ok(())
    }

    /// Handle agent completion with artifact validation
    fn handle_agent_completed(&mut self, task_id: &str) -> anyhow::Result<()> {
        // Get task info first (immutable borrow)
        let task_info = self.tasks.iter().find(|t| t.id == task_id).map(|t| {
            (
                t.status,
                t.title.clone(),
                t.worktree.clone(),
            )
        });

        let Some((status, title, worktree)) = task_info else {
            return Ok(());
        };

        match status {
            TaskStatus::Planning => {
                // Validate: Plan file must exist
                if !self.plan_manager.plan_file_exists(task_id) {
                    self.status_message = Some(format!(
                        "⚠️ Planner finished but no plan file found for '{}'",
                        title
                    ));
                    return Ok(());
                }

                // Plan file exists, advance to PlanReview
                if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                    task.set_status(TaskStatus::PlanReview);
                    self.store.save(&self.tasks)?;
                }

                // Auto-start executor with default
                let default_executor = self.orchestrator.default_executor.clone();
                self.start_executor_for_task(task_id, &default_executor)?;
            }
            TaskStatus::InProgress => {
                // Validate: Changes or commits must exist
                if let Some(worktree_path) = worktree {
                    let validator = WorktreeValidator::new(PathBuf::from(&worktree_path));

                    // Check for changes (commits or uncommitted)
                    let has_commits = validator.has_new_commits("main").unwrap_or(false);
                    let has_changes = validator.has_changes().unwrap_or(false);

                    if !has_commits && !has_changes {
                        self.status_message = Some(format!(
                            "⚠️ Executor finished but no changes found for '{}'",
                            title
                        ));
                        return Ok(());
                    }

                    // Changes exist, advance to Review
                    if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                        task.set_status(TaskStatus::Review);
                        self.store.save(&self.tasks)?;

                        let msg = if has_changes && !has_commits {
                            format!("✅ Implementation completed (uncommitted): {}", title)
                        } else {
                            format!("✅ Implementation completed: {}", title)
                        };
                        self.status_message = Some(msg);
                    }
                } else {
                    self.status_message = Some(format!(
                        "⚠️ No worktree found for '{}'",
                        title
                    ));
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Update running agent count
    async fn update_running_count(&mut self) {
        let runner = self.agent_runner.lock().await;
        self.running_count = runner.running_count();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut app = App::new()?;

    loop {
        // Process agent events (non-blocking)
        app.process_agent_events().await?;
        // Update running count
        app.update_running_count().await;

        terminal.draw(|frame| ui(frame, &app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match &app.input_mode {
                        InputMode::Normal => match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('h') | KeyCode::Left => app.move_left(),
                            KeyCode::Char('l') | KeyCode::Right => app.move_right(),
                            KeyCode::Char('k') | KeyCode::Up => app.move_up(),
                            KeyCode::Char('j') | KeyCode::Down => app.move_down(),
                            KeyCode::Char('n') => app.start_new_task(),
                            KeyCode::Char('a') => app.start_assign_agent(),
                            KeyCode::Enter => app.show_task_detail(),
                            KeyCode::Char('d') => {
                                app.show_diff()?;
                            }
                            KeyCode::Char('s') => app.stop_agent(),
                            KeyCode::Char('m') | KeyCode::Tab => {
                                app.move_task_forward()?;
                            }
                            KeyCode::Char('M') | KeyCode::BackTab => {
                                app.move_task_backward()?;
                            }
                            KeyCode::Char('g') => app.start_merge(),
                            KeyCode::Char('p') => {
                                app.create_pr()?;
                            }
                            KeyCode::Char('x') | KeyCode::Delete => {
                                app.delete_task()?;
                            }
                            _ => {}
                        },
                        InputMode::NewTaskTitle | InputMode::NewTaskDescription => match key.code {
                            KeyCode::Enter => app.confirm_input()?,
                            KeyCode::Esc => app.cancel_input(),
                            KeyCode::Backspace => app.handle_backspace(),
                            KeyCode::Char(c) => app.handle_input(c),
                            _ => {}
                        },
                        InputMode::SelectPlanner | InputMode::SelectExecutor => match key.code {
                            KeyCode::Enter => app.confirm_input()?,
                            KeyCode::Esc => app.cancel_input(),
                            KeyCode::Char('k') | KeyCode::Up => app.selection_up(),
                            KeyCode::Char('j') | KeyCode::Down => app.selection_down(),
                            _ => {}
                        },
                        InputMode::TaskDetail => match key.code {
                            KeyCode::Esc | KeyCode::Enter => app.cancel_input(),
                            KeyCode::Char('s') => {
                                app.stop_agent();
                                app.cancel_input();
                            }
                            KeyCode::Char('d') => {
                                app.cancel_input();
                                app.show_diff()?;
                            }
                            _ => {}
                        },
                        InputMode::ViewDiff => match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => app.cancel_input(),
                            KeyCode::Char('j') | KeyCode::Down => app.scroll_diff(1),
                            KeyCode::Char('k') | KeyCode::Up => app.scroll_diff(-1),
                            KeyCode::Char(' ') | KeyCode::PageDown => {
                                for _ in 0..10 { app.scroll_diff(1); }
                            }
                            _ => {}
                        },
                        InputMode::ConfirmMerge => match key.code {
                            KeyCode::Char('y') | KeyCode::Enter => app.execute_merge()?,
                            KeyCode::Char('n') | KeyCode::Esc => app.cancel_input(),
                            _ => {}
                        },
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(0),     // Kanban
            Constraint::Length(8),  // Log panel
            Constraint::Length(3),  // Footer
        ])
        .split(area);

    // Header
    let task_count = app.tasks.len();
    let running_indicator = if app.running_count > 0 {
        format!(" 🚀{} running ", app.running_count)
    } else {
        String::new()
    };
    let header_text = format!(" HIVE - AI Agent Kanban  ({} tasks){}", task_count, running_indicator);
    let header = Paragraph::new(header_text)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, main_layout[0]);

    // Kanban
    let kanban_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(main_layout[1]);

    let columns = [
        ("📋 Todo", Color::Yellow),
        ("🔄 Progress", Color::Blue),
        ("👀 Review", Color::Magenta),
        ("✅ Done", Color::Green),
    ];

    for (i, ((title, color), col_area)) in columns.iter().zip(kanban_layout.iter()).enumerate() {
        let is_selected = i == app.selected_column;
        let tasks = app.tasks_in_column(i);

        let items: Vec<ListItem> = tasks
            .iter()
            .enumerate()
            .map(|(j, task)| {
                let style = if is_selected && j == app.selected_task[i] {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };
                // Status icon (sub-status display for Progress column)
                let status_icon = task.status.icon();
                // Planner/Executor icon
                let agent_icon = if let Some(exec) = &task.executor {
                    match exec.as_str() {
                        "claude" => " 🤖",
                        _ => "",
                    }
                } else if let Some(planner) = &task.planner {
                    match planner.as_str() {
                        "gemini" => " ✨",
                        "codex" => " 🔮",
                        _ => "",
                    }
                } else {
                    ""
                };
                ListItem::new(format!(" {} {}{}", status_icon, task.title, agent_icon)).style(style)
            })
            .collect();

        let border_style = if is_selected {
            Style::default().fg(*color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let list = List::new(items).block(
            Block::default()
                .title(format!("{} ({})", title, tasks.len()))
                .borders(Borders::ALL)
                .border_style(border_style),
        );

        frame.render_widget(list, *col_area);
    }

    // Log panel
    let log_lines: Vec<Line> = app
        .agent_logs
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(|entry| {
            Line::from(vec![
                Span::styled(
                    format!("[{}] ", entry.task_title.chars().take(15).collect::<String>()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(&entry.line),
            ])
        })
        .collect();

    let log_panel = Paragraph::new(log_lines)
        .block(
            Block::default()
                .title(" 📜 Agent Logs ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(ratatui::widgets::Wrap { trim: true });
    frame.render_widget(log_panel, main_layout[2]);

    // Footer
    let footer_text = match &app.input_mode {
        InputMode::Normal => app
            .status_message
            .as_deref()
            .unwrap_or(" [n]ew [a]ssign [d]iff [p]r [m]ove [g]merge [s]top [x]del [q]uit "),
        _ => app.status_message.as_deref().unwrap_or(""),
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, main_layout[3]);

    // Show popup in input mode
    match app.input_mode {
        InputMode::NewTaskTitle | InputMode::NewTaskDescription => {
            let popup_area = centered_rect(50, 20, area);
            frame.render_widget(Clear, popup_area);

            let title = match app.input_mode {
                InputMode::NewTaskTitle => "New Task - Title",
                InputMode::NewTaskDescription => "New Task - Description",
                _ => "",
            };

            let input = Paragraph::new(app.input_buffer.as_str())
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                );
            frame.render_widget(input, popup_area);
        }
        InputMode::SelectPlanner | InputMode::SelectExecutor => {
            let popup_area = centered_rect(50, 35, area);
            frame.render_widget(Clear, popup_area);

            let (title, color) = match app.input_mode {
                InputMode::SelectPlanner => ("🧠 Select Planner", Color::Yellow),
                InputMode::SelectExecutor => ("🔨 Select Executor", Color::Cyan),
                _ => ("Select", Color::White),
            };

            let items: Vec<ListItem> = app
                .selection_list
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let (icon, desc) = match name.as_str() {
                        "gemini" => ("✨", "Fast & cheap. For general tasks"),
                        "codex" => ("🔮", "Strong reasoning. For complex design"),
                        "claude" => ("🤖", "High code quality. Best for impl"),
                        _ => ("•", ""),
                    };
                    let style = if i == app.selected_index {
                        Style::default().bg(color).fg(Color::Black)
                    } else {
                        Style::default()
                    };
                    ListItem::new(format!(" {} {} - {}", icon, name, desc)).style(style)
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(color)),
            );
            frame.render_widget(list, popup_area);
        }
        InputMode::TaskDetail => {
            if let Some(task) = app.selected_task() {
                let popup_area = centered_rect(60, 50, area);
                frame.render_widget(Clear, popup_area);

                let mut lines: Vec<Line> = vec![];
                lines.push(Line::from(vec![
                    Span::styled("Title: ", Style::default().fg(Color::Gray)),
                    Span::styled(&task.title, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::Gray)),
                    Span::styled(format!("{} {}", task.status.icon(), task.status.display_name()), Style::default().fg(Color::Cyan)),
                ]));
                if !task.description.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("Description: ", Style::default().fg(Color::Gray)),
                        Span::styled(&task.description, Style::default().fg(Color::White)),
                    ]));
                }
                lines.push(Line::from(""));
                if let Some(planner) = &task.planner {
                    lines.push(Line::from(vec![
                        Span::styled("Planner: ", Style::default().fg(Color::Gray)),
                        Span::styled(format!("✨ {}", planner), Style::default().fg(Color::Yellow)),
                    ]));
                }
                if let Some(executor) = &task.executor {
                    lines.push(Line::from(vec![
                        Span::styled("Executor: ", Style::default().fg(Color::Gray)),
                        Span::styled(format!("🤖 {}", executor), Style::default().fg(Color::Green)),
                    ]));
                }
                if let Some(branch) = &task.branch {
                    lines.push(Line::from(vec![
                        Span::styled("Branch: ", Style::default().fg(Color::Gray)),
                        Span::styled(branch, Style::default().fg(Color::Magenta)),
                    ]));
                }
                if let Some(worktree) = &task.worktree {
                    lines.push(Line::from(vec![
                        Span::styled("Worktree: ", Style::default().fg(Color::Gray)),
                        Span::styled(worktree, Style::default().fg(Color::Blue)),
                    ]));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("ID: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&task.id, Style::default().fg(Color::DarkGray)),
                ]));

                let detail = Paragraph::new(lines)
                    .block(
                        Block::default()
                            .title("📋 Task Detail")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Cyan)),
                    );
                frame.render_widget(detail, popup_area);
            }
        }
        InputMode::ViewDiff => {
            let popup_area = centered_rect(80, 80, area);
            frame.render_widget(Clear, popup_area);

            let lines: Vec<Line> = app.diff_content
                .lines()
                .skip(app.diff_scroll)
                .take(popup_area.height as usize - 2)
                .map(|line| {
                    let style = if line.starts_with('+') && !line.starts_with("+++") {
                        Style::default().fg(Color::Green)
                    } else if line.starts_with('-') && !line.starts_with("---") {
                        Style::default().fg(Color::Red)
                    } else if line.starts_with("@@") {
                        Style::default().fg(Color::Cyan)
                    } else if line.starts_with("diff") || line.starts_with("index") {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::styled(line, style)
                })
                .collect();

            let total_lines = app.diff_content.lines().count();
            let title = format!("📄 Diff ({}/{} lines) [j/k scroll, ESC close]", app.diff_scroll + 1, total_lines);

            let diff_view = Paragraph::new(lines)
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow)),
                );
            frame.render_widget(diff_view, popup_area);
        }
        InputMode::ConfirmMerge => {
            if let Some(task) = app.selected_task() {
                let popup_area = centered_rect(50, 25, area);
                frame.render_widget(Clear, popup_area);

                let lines = vec![
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Merge ", Style::default().fg(Color::White)),
                        Span::styled(&task.title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(" to main?", Style::default().fg(Color::White)),
                    ]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Branch: ", Style::default().fg(Color::Gray)),
                        Span::styled(task.branch.as_deref().unwrap_or("unknown"), Style::default().fg(Color::Magenta)),
                    ]),
                    Line::from(""),
                    Line::styled("[y] Yes  [n] No", Style::default().fg(Color::Yellow)),
                ];

                let confirm = Paragraph::new(lines)
                    .alignment(Alignment::Center)
                    .block(
                        Block::default()
                            .title("🔀 Confirm Merge")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Yellow)),
                    );
                frame.render_widget(confirm, popup_area);
            }
        }
        InputMode::Normal => {}
    }
}

/// Calculate centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
