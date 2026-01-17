# Hive

> AI Agent Orchestration TUI - Vibe Kanban for terminal

Hive is a terminal-based Kanban board integrated with AI agents to orchestrate software development tasks. It seamlessly combines task management, git worktrees, and AI-driven planning and execution.

## Features

- **TUI Kanban Board**: Visualize tasks in Todo, Progress, Review, and Done columns.
- **AI Agent Orchestration**: Assign "Planner" and "Executor" agents (e.g., Gemini, Claude, Codex) to tasks.
- **Git Integration**: Automatically manages git worktrees and branches for each task.
- **Diff Viewer**: Integrated git diff viewer.
- **PR Creation**: Create GitHub Pull Requests directly from the TUI.

## Prerequisites

- **Rust**: `cargo` is required to build the project.
- **Git**: Version control system.
- **GitHub CLI (`gh`)**: Required for creating Pull Requests.
- **Configuration**: A `.hive` directory is expected in the repository root.

## Installation

Clone the repository and install using Cargo:

```bash
cargo install --path .
```

Or build manually:

```bash
cargo build --release
```

## Usage

Start the application from your project root:

```bash
hive
```

### Workflow

1.  **Create Task**: Press `n` to create a new task with a title and description.
2.  **Assign Planner**: Select the task and press `a` to assign a Planner agent. The agent will:
    -   Validate the environment.
    -   Create a new git worktree and branch.
    -   Generate an implementation plan saved to `.hive/plans/`.
3.  **Review Plan**: Once planning is complete, the task moves to "Plan Review".
4.  **Assign Executor**: Press `a` again to assign an Executor agent. The agent will write code based on the generated plan.
5.  **Review Implementation**:
    *   Press `d` to view the diff.
    *   Press `Enter` to see task details.
6.  **Merge / PR**:
    *   If satisfied, press `p` to create a Pull Request.
    *   Or press `g` to merge directly to main.

## Configuration

Hive looks for a `.hive` directory in the project root.

-   **Plans**: Saved in `.hive/plans/`
-   **Logs**: Agent logs are saved in `.hive/logs/`
-   **Config**: Optional `.hive/config.json` to override default agents.

### Default Agents

-   **Planner**: `gemini` (Gemini CLI)
-   **Executor**: `claude` (Claude Code)

## Keybindings

### Normal Mode

| Key | Action |
| --- | --- |
| `q` | Quit |
| `h` / `Left` | Move column left |
| `l` / `Right` | Move column right |
| `k` / `Up` | Move task up |
| `j` / `Down` | Move task down |
| `n` | New task |
| `a` | Assign agent (Planner/Executor) |
| `Enter` | Show task detail |
| `d` | Show diff |
| `s` | Stop agent |
| `m` / `Tab` | Move task forward |
| `M` / `BackTab` | Move task backward |
| `g` | Start merge (only from Review status) |
| `p` | Create PR (only from Review status) |
| `x` / `Delete` | Delete task |

### Task Detail

| Key | Action |
| --- | --- |
| `Esc` / `Enter` | Close popup |
| `s` | Stop agent |
| `d` | Show diff |

### Diff Viewer

| Key | Action |
| --- | --- |
| `Esc` / `q` | Close viewer |
| `j` / `k` | Scroll down/up |
| `Space` | Scroll page down |
