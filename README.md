# GitLab Search CLI

A command-line interface for searching code in GitLab repositories.

## Features

- Configure multiple GitLab instances
- List projects in GitLab instances
- Search for code in specific projects or across all projects
- Colorized output for better readability
- Progress bar for search operations

## Installation

### Prerequisites

- Rust and Cargo (install from [rustup.rs](https://rustup.rs/))

### Building from source

```bash
# Clone the repository
git clone https://github.com/shkmv/gitlab-search-ui.git
cd gitlab-search-ui/gitlab-search-cli

# Build the project
cargo build --release

# The binary will be available at target/release/gitlab-search-cli
```

## Usage

### Configuration

Before using the tool, you need to configure at least one GitLab instance:

```bash
# Add a new GitLab instance
gitlab-search-cli config --name my-gitlab --url https://gitlab.example.com --token your-personal-access-token

# List configured instances
gitlab-search-cli config --list
```

### Listing Projects

```bash
# List projects in the default GitLab instance
gitlab-search-cli projects

# List projects in a specific GitLab instance
gitlab-search-cli projects --instance my-gitlab

# Include archived projects
gitlab-search-cli projects --archived
```

### Searching Code

```bash
# Search in a specific project (by ID or path with namespace)
gitlab-search-cli search --query "your search query" --project 123
gitlab-search-cli search --query "your search query" --project group/project-name

# Search in all projects (may be slow for large GitLab instances)
gitlab-search-cli search --query "your search query" --all-projects

# Search in a specific GitLab instance
gitlab-search-cli search --query "your search query" --instance my-gitlab --project 123
```

## Getting Help

```bash
# Show general help
gitlab-search-cli --help

# Show help for a specific command
gitlab-search-cli config --help
gitlab-search-cli projects --help
gitlab-search-cli search --help
```

## Personal Access Token

To use this tool, you need a GitLab personal access token with the `read_api` scope. You can create one in your GitLab account under Settings > Access Tokens.
