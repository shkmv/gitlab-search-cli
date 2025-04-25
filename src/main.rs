#[allow(dead_code)]
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use config::{Config, File};
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Configure GitLab instances
    Config {
        /// GitLab instance name
        #[arg(short, long)]
        name: Option<String>,

        /// GitLab URL
        #[arg(short, long)]
        url: Option<String>,

        /// GitLab API token
        #[arg(short, long)]
        token: Option<String>,

        /// List all configured GitLab instances
        #[arg(short, long)]
        list: bool,
    },
    /// Search for code in GitLab projects
    Search {
        /// Search query
        #[arg(short, long)]
        query: String,

        /// GitLab instance name (from config)
        #[arg(short, long)]
        instance: Option<String>,

        /// Project ID or path with namespace
        #[arg(short, long)]
        project: Option<String>,

        /// Search in all projects (may be slow)
        #[arg(short, long)]
        all_projects: bool,
    },
    /// List projects in GitLab instance
    Projects {
        /// GitLab instance name (from config)
        #[arg(short, long)]
        instance: Option<String>,

        /// Include archived projects
        #[arg(short, long)]
        archived: bool,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct GitLabConfig {
    name: String,
    url: String,
    token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    gitlab_instances: Vec<GitLabConfig>,
}

#[derive(Debug, Deserialize)]
struct GitLabVersion {
    version: String,
    revision: String,
}

#[derive(Debug, Deserialize, Clone)]
struct Namespace {
    id: u64,
    name: String,
    path: String,
    kind: String,
    full_path: String,
    parent_id: Option<u64>,
    web_url: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone)]
struct Project {
    id: u64,
    description: Option<String>,
    name: String,
    name_with_namespace: String,
    path: String,
    path_with_namespace: String,
    created_at: String,
    web_url: String,
    last_activity_at: String,
    namespace: Namespace,
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    basename: String,
    data: String,
    path: String,
    filename: String,
    id: Option<u64>,
    ref_field: String,
    startline: u64,
    project_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchResultRaw {
    basename: String,
    data: String,
    path: String,
    filename: String,
    id: Option<u64>,
    #[serde(rename = "ref")]
    ref_field: String,
    startline: u64,
    project_id: u64,
}

async fn get_config() -> Result<AppConfig> {
    let config_dir = dirs::config_dir()
        .context("Could not find config directory")?
        .join("gitlab-search-cli");

    std::fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

    let config_path = config_dir.join("config.json");

    if !config_path.exists() {
        let default_config = AppConfig {
            gitlab_instances: Vec::new(),
        };
        let config_json = serde_json::to_string_pretty(&default_config)?;
        std::fs::write(&config_path, config_json).context("Failed to write default config")?;
    }

    let config = Config::builder()
        .add_source(File::from(config_path))
        .build()?;

    let app_config: AppConfig = config.try_deserialize()?;
    Ok(app_config)
}

async fn save_config(config: &AppConfig) -> Result<()> {
    let config_dir = dirs::config_dir()
        .context("Could not find config directory")?
        .join("gitlab-search-cli");

    let config_path = config_dir.join("config.json");
    let config_json = serde_json::to_string_pretty(&config)?;
    std::fs::write(&config_path, config_json).context("Failed to write config")?;
    Ok(())
}

async fn get_gitlab_version(
    client: &reqwest::Client,
    config: &GitLabConfig,
) -> Result<GitLabVersion> {
    let url = format!("{}/api/v4/version", config.url);
    let response = client
        .get(&url)
        .header("PRIVATE-TOKEN", &config.token)
        .send()
        .await?
        .error_for_status()?;

    let version: GitLabVersion = response.json().await?;
    Ok(version)
}

async fn get_projects(
    client: &reqwest::Client,
    config: &GitLabConfig,
    include_archived: bool,
) -> Result<Vec<Project>> {
    let mut all_projects = Vec::new();
    let mut page = 1;
    let per_page = 50;

    loop {
        let url = format!("{}/api/v4/projects", config.url);
        let request = client
            .get(&url)
            .header("PRIVATE-TOKEN", &config.token)
            .query(&[
                ("simple", "true"),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
                ("order_by", "id"),
                ("membership", "true"),
                ("archived", &include_archived.to_string()),
            ]);

        let response = request.send().await?.error_for_status()?;
        let projects: Vec<Project> = response.json().await?;

        if projects.is_empty() {
            break;
        }

        all_projects.extend(projects);
        page += 1;
    }

    Ok(all_projects)
}

async fn search_project_blobs(
    client: &reqwest::Client,
    config: &GitLabConfig,
    project_id: u64,
    query: &str,
) -> Result<Vec<SearchResultRaw>> {
    let url = format!("{}/api/v4/projects/{}/search", config.url, project_id);
    let response = client
        .get(&url)
        .header("PRIVATE-TOKEN", &config.token)
        .query(&[("scope", "blobs"), ("search", query), ("per_page", "100")])
        .send()
        .await?
        .error_for_status()?;

    let results: Vec<SearchResultRaw> = response.json().await?;
    Ok(results)
}

async fn handle_config_command(
    name: Option<String>,
    url: Option<String>,
    token: Option<String>,
    list: bool,
) -> Result<()> {
    let mut config = get_config().await?;

    if list {
        println!("Configured GitLab instances:");
        if config.gitlab_instances.is_empty() {
            println!("  No instances configured");
        } else {
            for instance in &config.gitlab_instances {
                println!("  {} - {}", instance.name.green(), instance.url);
            }
        }
        return Ok(());
    }

    let has_name = name.is_some();
    let has_url = url.is_some();
    let has_token = token.is_some();

    if let (Some(name_val), Some(url_val), Some(token_val)) = (name, url, token) {
        if let Some(pos) = config
            .gitlab_instances
            .iter()
            .position(|i| i.name == name_val)
        {
            config.gitlab_instances[pos] = GitLabConfig {
                name: name_val.clone(),
                url: url_val.clone(),
                token: token_val.clone(),
            };
            println!("Updated GitLab instance: {}", name_val.green());
        } else {
            config.gitlab_instances.push(GitLabConfig {
                name: name_val.clone(),
                url: url_val.clone(),
                token: token_val.clone(),
            });
            println!("Added new GitLab instance: {}", name_val.green());
        }

        save_config(&config).await?;

        let client = reqwest::Client::new();
        let instance = config
            .gitlab_instances
            .iter()
            .find(|i| i.name == name_val)
            .unwrap();

        match get_gitlab_version(&client, instance).await {
            Ok(version) => {
                println!(
                    "Successfully connected to GitLab instance: {} (version: {})",
                    name_val.green(),
                    version.version.cyan()
                );
            }
            Err(e) => {
                println!(
                    "Failed to connect to GitLab instance: {} - Error: {}",
                    name_val.red(),
                    e
                );
            }
        }
    } else if has_name || has_url || has_token {
        println!(
            "{}",
            "To configure a GitLab instance, you must provide name, url, and token".red()
        );
    } else {
        println!(
            "{}",
            "Use --list to see configured instances or provide --name, --url, and --token to add/update an instance"
                .yellow()
        );
    }

    Ok(())
}

async fn handle_projects_command(instance: Option<String>, archived: bool) -> Result<()> {
    let config = get_config().await?;

    let instance_config = if let Some(instance_name) = instance {
        config
            .gitlab_instances
            .iter()
            .find(|i| i.name == instance_name)
            .with_context(|| format!("GitLab instance '{}' not found in config", instance_name))?
    } else if !config.gitlab_instances.is_empty() {
        &config.gitlab_instances[0]
    } else {
        return Err(anyhow::anyhow!(
            "No GitLab instances configured. Use 'config' command to add one."
        ));
    };

    println!(
        "Fetching projects from GitLab instance: {}",
        instance_config.name.green()
    );

    let client = reqwest::Client::new();
    let projects = get_projects(&client, instance_config, archived).await?;

    println!("Found {} projects:", projects.len());
    for project in projects {
        println!(
            "  {} (ID: {}) - {}",
            project.name_with_namespace.green(),
            project.id.to_string().cyan(),
            project.web_url
        );
    }

    Ok(())
}

async fn handle_search_command(
    query: String,
    instance: Option<String>,
    project: Option<String>,
    all_projects: bool,
) -> Result<()> {
    let config = get_config().await?;

    let instance_config = if let Some(instance_name) = instance {
        config
            .gitlab_instances
            .iter()
            .find(|i| i.name == instance_name)
            .with_context(|| format!("GitLab instance '{}' not found in config", instance_name))?
    } else if !config.gitlab_instances.is_empty() {
        &config.gitlab_instances[0]
    } else {
        return Err(anyhow::anyhow!(
            "No GitLab instances configured. Use 'config' command to add one."
        ));
    };

    println!(
        "Searching in GitLab instance: {}",
        instance_config.name.green()
    );

    let client = reqwest::Client::new();

    let projects_to_search = if let Some(project_id_or_path) = project {
        if let Ok(project_id) = project_id_or_path.parse::<u64>() {
            vec![Project {
                id: project_id,
                description: None,
                name: project_id_or_path.clone(),
                name_with_namespace: project_id_or_path.clone(),
                path: project_id_or_path.clone(),
                path_with_namespace: project_id_or_path,
                created_at: String::new(),
                web_url: String::new(),
                last_activity_at: String::new(),
                namespace: Namespace {
                    id: 0,
                    name: String::new(),
                    path: String::new(),
                    kind: String::new(),
                    full_path: String::new(),
                    parent_id: None,
                    web_url: String::new(),
                },
            }]
        } else {
            let all_projects = get_projects(&client, instance_config, false).await?;
            all_projects
                .into_iter()
                .filter(|p| p.path_with_namespace == project_id_or_path)
                .collect()
        }
    } else if all_projects {
        println!("Fetching all projects...");
        get_projects(&client, instance_config, false).await?
    } else {
        return Err(anyhow::anyhow!(
            "You must specify a project with --project or use --all-projects to search in all projects"
        ));
    };

    if projects_to_search.is_empty() {
        return Err(anyhow::anyhow!("No projects found to search in"));
    }

    println!("Searching for: {}", query.cyan());
    println!("Searching in {} projects...", projects_to_search.len());

    let pb = ProgressBar::new(projects_to_search.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let results = Arc::new(Mutex::new(Vec::new()));
    let tasks = projects_to_search.iter().map(|project| {
        let client = client.clone();
        let config = instance_config.clone();
        let query = query.clone();
        let project_id = project.id;
        let project_name = project.name_with_namespace.clone();
        let results = Arc::clone(&results);
        let pb = pb.clone();

        async move {
            match search_project_blobs(&client, &config, project_id, &query).await {
                Ok(project_results) => {
                    let mut results_guard = results.lock().await;
                    for result in project_results {
                        results_guard.push((project_name.clone(), result));
                    }
                }
                Err(e) => {
                    eprintln!("Error searching in project {}: {}", project_name, e);
                }
            }
            pb.inc(1);
            pb.set_message(format!("Searching in {}", project_name));
        }
    });

    join_all(tasks).await;
    pb.finish_with_message("Search completed");

    let search_results = results.lock().await;
    println!("\nFound {} results:", search_results.len());

    for (project_name, result) in search_results.iter() {
        println!(
            "\n{} - {}:{}",
            project_name.green(),
            result.path.cyan(),
            result.startline.to_string().yellow()
        );

        let lines = result.data.lines();
        for (i, line) in lines.enumerate() {
            println!(
                "{}: {}",
                (result.startline + i as u64).to_string().yellow(),
                line
            );
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Config {
            name,
            url,
            token,
            list,
        } => {
            handle_config_command(name.clone(), url.clone(), token.clone(), *list).await?;
        }
        Commands::Search {
            query,
            instance,
            project,
            all_projects,
        } => {
            handle_search_command(
                query.clone(),
                instance.clone(),
                project.clone(),
                *all_projects,
            )
            .await?;
        }
        Commands::Projects { instance, archived } => {
            handle_projects_command(instance.clone(), *archived).await?;
        }
    }

    Ok(())
}
