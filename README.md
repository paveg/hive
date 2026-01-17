# HIVE - AI Agent Orchestration TUI

> AI Agent Orchestration TUI - Vibe Kanban for terminal

Hive is a terminal-based Kanban board that orchestrates AI agents to help you plan and execute tasks. It integrates directly with Git to manage worktrees, branches, and Pull Requests, providing a seamless workflow for AI-assisted development.

## Features

- **Kanban Workflow**: Manage tasks across Todo, Progress, Review, and Done columns.
- **AI Orchestration**:
  - **Planners**: AI agents (Gemini, Codex, Claude) that break down tasks into actionable plans.
  - **Executors**: AI agents that implement the plans in a dedicated Git worktree.
- **Git Integration**:
  - Automated worktree creation for each task.
  - Branch management and context switching.
  - Built-in Diff viewer.
  - Merge conflict detection and resolution.
  - One-key Pull Request creation via GitHub CLI (`gh`).
- **Terminal UI**: Fast, keyboard-centric interface built with `ratatui`.

## Prerequisites

- **Rust**: [Install Rust](https://www.rust-lang.org/tools/install)
- **Git**: Version control system.
- **GitHub CLI (`gh`)**: Required for creating Pull Requests. [Install gh](https://cli.github.com/)

## Installation

```bash
cargo install --path .
```

## Usage

Start the application:

```bash
hive
```

### Workflow

1.  **Create Task**: Press `n` to create a new task. Enter a title and optional description.
2.  **Plan**: The default Planner agent (e.g., Gemini) will automatically start creating a plan.
3.  **Review Plan**: Once the planner finishes, the task moves to **Progress**. You can assign an Executor agent (e.g., Claude) by pressing `a`.
4.  **Execution**: The Executor agent implements the plan in a dedicated worktree.
5.  **Review Code**: When execution is complete, the task moves to **Review**.
    - Press `d` to view the diff.
    - Press `p` to push the branch and create a PR.
    - Press `g` to merge locally.
6.  **Done**: Merged tasks move to **Done**.

## Keybindings

### Global / Navigation

| Key | Action |
| --- | --- |
| `q` | Quit application |
| `h` / `Left` | Move selection left (Column) |
| `l` / `Right` | Move selection right (Column) |
| `k` / `Up` | Move selection up (Task) |
| `j` / `Down` | Move selection down (Task) |
| `Enter` | Show task details |

### Task Management

| Key | Action |
| --- | --- |
| `n` | Create new task |
| `x` / `Del` | Delete task |
| `m` / `Tab` | Move task forward (Next status) |
| `M` / `BackTab` | Move task backward (Previous status) |

### AI & Actions

| Key | Action |
| --- | --- |
| `a` | Assign agent (Planner/Executor) |
| `s` | Stop running agent |
| `d` | Show diff (Review status) |
| `p` | Create PR (Review status) |
| `g` | Merge to main (Review status) |

### Inputs / Dialogs

| Key | Action |
| --- | --- |
| `Enter` | Confirm input / Select item |
| `Ctrl+Enter` | Insert newline (Description) |
| `Esc` | Cancel / Close dialog |

## Configuration

Hive uses a `.hive` directory in the repository root for storing configuration and agent plans.

## License

MIT
