// src/utils/db/db_reader.rs
use crate::data::client_repositories::ClientRepositories;
use crate::data::repository::Repository;
use crate::interface::help_prompt::{ConfigurationDoc, Onboarding};
use crate::utils::is_test_mode;
use anyhow::{Context, Result};
use directories::ProjectDirs;
use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::path::PathBuf;

const DB_FILE_NAME: &str = "autolog.db";

/// Get the platform-specific path for the database
pub fn get_db_path() -> PathBuf {
    if is_test_mode() {
        PathBuf::from("./testing-utils").join(DB_FILE_NAME)
    } else {
        let proj_dirs = ProjectDirs::from("dev", "autolog", "autolog-cli")
            .expect("Failed to determine app data directory");

        let data_dir = proj_dirs.data_dir();
        // Ensure directory exists
        fs::create_dir_all(data_dir).expect("Failed to create data directory");

        data_dir.join(DB_FILE_NAME)
    }
}

/// Get a database connection
pub fn get_connection() -> Result<Connection> {
    let db_path = get_db_path();
    let conn =
        Connection::open(&db_path).context(format!("Failed to open database at {:?}", db_path))?;

    // Initialize schema if needed
    init_schema(&conn).context("Failed to initialize database schema")?;

    Ok(conn)
}

/// Initialize the database schema if tables don't exist
fn init_schema(conn: &Connection) -> Result<()> {
    // Create clients table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS clients (
            id TEXT PRIMARY KEY,
            client_name TEXT NOT NULL,
            client_address TEXT,
            client_contact_person TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .context("Failed to create clients table")?;

    // Create users table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT,
            is_alias INTEGER NOT NULL DEFAULT 0,
            thumbnail TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )
    .context("Failed to create users table")?;

    // Create repositories table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS repositories (
            id TEXT PRIMARY KEY,
            namespace TEXT,
            namespace_alias TEXT,
            repo_path TEXT,
            git_path TEXT,
            user_id TEXT,
            name TEXT,
            email TEXT,
            client_id TEXT,
            client_name TEXT,
            client_contact_person TEXT,
            client_address TEXT,
            project_number TEXT,
            service TEXT,
            service_username TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (client_id) REFERENCES clients (id)
        )",
        [],
    )
    .context("Failed to create repositories table")?;

    // Create git_log_dates table with proper structure for HashMap<i32, HashMap<u32, HashSet<u32>>>
    conn.execute(
        "CREATE TABLE IF NOT EXISTS git_log_years (
            repository_id TEXT NOT NULL,
            year INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (repository_id, year),
            FOREIGN KEY (repository_id) REFERENCES repositories (id)
        )",
        [],
    )
    .context("Failed to create git_log_years table")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS git_log_months (
            repository_id TEXT NOT NULL,
            year INTEGER NOT NULL,
            month INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (repository_id, year, month),
            FOREIGN KEY (repository_id, year) REFERENCES git_log_years (repository_id, year)
        )",
        [],
    )
    .context("Failed to create git_log_months table")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS git_log_days (
            repository_id TEXT NOT NULL,
            year INTEGER NOT NULL,
            month INTEGER NOT NULL,
            day INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (repository_id, year, month, day),
            FOREIGN KEY (repository_id, year, month) REFERENCES git_log_months (repository_id, year, month)
        )",
        [],
    ).context("Failed to create git_log_days table")?;

    // Create client_repositories table (for joining clients and users with approval info)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS client_repositories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            client_id TEXT NOT NULL,
            user_id TEXT,
            requires_approval INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (client_id) REFERENCES clients (id),
            FOREIGN KEY (user_id) REFERENCES users (id)
        )",
        [],
    )
    .context("Failed to create client_repositories table")?;

    // Create approvers table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS approvers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            client_repository_id INTEGER NOT NULL,
            approvers_name TEXT,
            approvers_email TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (client_repository_id) REFERENCES client_repositories (id)
        )",
        [],
    )
    .context("Failed to create approvers table")?;

    // Additional timesheet-related tables would go here

    Ok(())
}

/// Read data from the database or call onboarding if database is empty
pub fn read_data_from_db<T>(
    buffer: &mut String,
    prompt: &mut T,
) -> Result<(), Box<dyn std::error::Error>>
where
    T: Onboarding,
{
    let conn = get_connection()?;

    // Check if we have any data
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM clients", [], |row| row.get(0))
        .unwrap_or(0);

    if count == 0 {
        // No data exists, call onboarding
        prompt.onboarding(true)?;
        return Ok(());
    }

    // Load data into ConfigurationDoc
    let config_doc = load_config_doc(&conn)?;

    // Convert to JSON string for compatibility with existing code
    *buffer = serde_json::to_string(&config_doc)?;

    Ok(())
}

/// Delete the database file (equivalent to delete_config_file)
pub fn delete_db() -> Result<(), Box<dyn std::error::Error>> {
    if is_test_mode() {
        return Ok(());
    }

    let db_path = get_db_path();
    if db_path.exists() {
        std::fs::remove_file(db_path)?;
    }

    Ok(())
}

/// Write config to database (replacement for write_json_to_config_file)
pub fn write_config_to_db(json: String) -> Result<(), Box<dyn std::error::Error>> {
    if is_test_mode() {
        // For tests, we can just return OK
        return Ok(());
    }

    // Parse the JSON to ConfigurationDoc
    let config_doc: ConfigurationDoc = serde_json::from_str(&json)?;

    // Get database connection
    let mut conn = get_connection()?;

    // Use a transaction for atomicity
    let tx = conn.transaction()?;

    // Write each client repository to the database
    for client_repo in config_doc.iter() {
        save_client_repository(&tx, client_repo)?;
    }

    // Commit the transaction
    tx.commit()?;

    Ok(())
}

/// Save a ClientRepositories object to the database
fn save_client_repository(
    tx: &rusqlite::Transaction,
    client_repo: &ClientRepositories,
) -> Result<(), Box<dyn std::error::Error>> {
    // Save client if present
    if let Some(client) = &client_repo.client {
        tx.execute(
            "INSERT OR REPLACE INTO clients (id, client_name, client_address, client_contact_person)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                client.id,
                client.client_name,
                client.client_address,
                client.client_contact_person
            ],
        )?;
    }

    // Save user if present
    if let Some(user) = &client_repo.user {
        tx.execute(
            "INSERT OR REPLACE INTO users (id, name, email, is_alias, thumbnail)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                user.id,
                user.name,
                user.email,
                user.is_alias as i32,
                user.thumbnail
            ],
        )?;
    }

    // Create client_repositories entry if both client and user exist
    if let (Some(client), Some(user)) = (&client_repo.client, &client_repo.user) {
        // Check if relation already exists
        let client_repo_id: Option<i64> = tx
            .query_row(
                "SELECT id FROM client_repositories
             WHERE client_id = ?1 AND user_id = ?2",
                params![client.id, user.id],
                |row| row.get(0),
            )
            .optional()?;

        let client_repo_id = if let Some(id) = client_repo_id {
            // Update existing relation
            tx.execute(
                "UPDATE client_repositories
                 SET requires_approval = ?1
                 WHERE id = ?2",
                params![client_repo.requires_approval.unwrap_or(false) as i32, id],
            )?;
            id
        } else {
            // Insert new relation
            tx.execute(
                "INSERT INTO client_repositories (client_id, user_id, requires_approval)
                 VALUES (?1, ?2, ?3)",
                params![
                    client.id,
                    user.id,
                    client_repo.requires_approval.unwrap_or(false) as i32
                ],
            )?;
            tx.last_insert_rowid()
        };

        // Save approver if it exists
        if let Some(approver) = &client_repo.approver {
            tx.execute(
                "INSERT OR REPLACE INTO approvers (client_repository_id, approvers_name, approvers_email)
                 VALUES (?1, ?2, ?3)",
                params![
                    client_repo_id,
                    approver.approvers_name,
                    approver.approvers_email
                ],
            )?;
        }
    }

    // Save repositories if they exist
    if let Some(repositories) = &client_repo.repositories {
        for repo in repositories {
            if let Some(id) = &repo.id {
                // Save repository
                tx.execute(
                    "INSERT OR REPLACE INTO repositories (
                        id, namespace, namespace_alias, repo_path, git_path,
                        user_id, name, email, client_id, client_name,
                        client_contact_person, client_address, project_number,
                        service, service_username
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    params![
                        id,
                        repo.namespace,
                        repo.namespace_alias,
                        repo.repo_path,
                        repo.git_path,
                        repo.user_id,
                        repo.name,
                        repo.email,
                        repo.client_id,
                        repo.client_name,
                        repo.client_contact_person,
                        repo.client_address,
                        repo.project_number,
                        repo.service,
                        repo.service_username
                    ],
                )?;

                // Save git log dates if present
                if let Some(git_log_dates) = &repo.git_log_dates {
                    for (&year, months) in git_log_dates.iter() {
                        // Insert year
                        tx.execute(
                            "INSERT OR REPLACE INTO git_log_years (repository_id, year)
                             VALUES (?1, ?2)",
                            params![id, year],
                        )?;

                        for (&month, days) in months.iter() {
                            // Insert month
                            tx.execute(
                                "INSERT OR REPLACE INTO git_log_months (repository_id, year, month)
                                 VALUES (?1, ?2, ?3)",
                                params![id, year, month],
                            )?;

                            for &day in days.iter() {
                                // Insert day
                                tx.execute(
                                    "INSERT OR REPLACE INTO git_log_days (repository_id, year, month, day)
                                     VALUES (?1, ?2, ?3, ?4)",
                                    params![id, year, month, day],
                                )?;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Load configuration document from database
fn load_config_doc(conn: &Connection) -> Result<ConfigurationDoc, Box<dyn std::error::Error>> {
    let mut result = Vec::new();

    // Get all clients
    let mut stmt = conn.prepare(
        "SELECT id, client_name, client_address, client_contact_person
         FROM clients",
    )?;

    let client_rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let client_name: String = row.get(1)?;
        let client_address: String = row.get(2)?;
        let client_contact_person: String = row.get(3)?;

        Ok((id, client_name, client_address, client_contact_person))
    })?;

    for client_result in client_rows {
        let (id, client_name, client_address, client_contact_person) = client_result?;

        // Create client
        let client = crate::data::client_repositories::Client {
            id: id.clone(),
            client_name,
            client_address,
            client_contact_person,
        };

        // Find user for this client
        let user = conn
            .query_row(
                "SELECT u.id, u.name, u.email, u.is_alias, u.thumbnail
                 FROM users u
                 JOIN client_repositories cr ON u.id = cr.user_id
                 WHERE cr.client_id = ?1
                 LIMIT 1",
                params![id],
                |row| {
                    Ok(Some(crate::data::client_repositories::User {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        email: row.get(2)?,
                        is_alias: row.get(3)?,
                        thumbnail: row.get(4)?,
                    }))
                },
            )
            .optional()?
            .flatten();

        // Find requires_approval flag
        let requires_approval: Option<bool> = conn
            .query_row(
                "SELECT requires_approval FROM client_repositories WHERE client_id = ?1 LIMIT 1",
                params![id],
                |row| {
                    let val: i32 = row.get(0)?;
                    Ok(Some(val != 0))
                },
            )
            .optional()?
            .flatten();

        // Find approver
        let approver = conn
            .query_row(
                "SELECT a.approvers_name, a.approvers_email
                 FROM approvers a
                 JOIN client_repositories cr ON a.client_repository_id = cr.id
                 WHERE cr.client_id = ?1
                 LIMIT 1",
                params![id],
                |row| {
                    Ok(Some(crate::data::client_repositories::Approver {
                        approvers_name: row.get(0)?,
                        approvers_email: row.get(1)?,
                    }))
                },
            )
            .optional()?
            .flatten();

        // Find repositories for this client
        let mut repo_stmt = conn.prepare(
            "SELECT id, namespace, namespace_alias, repo_path, git_path,
                    user_id, name, email, client_id, client_name,
                    client_contact_person, client_address, project_number,
                    service, service_username
             FROM repositories
             WHERE client_id = ?1",
        )?;

        let repo_rows = repo_stmt.query_map(params![id], |row| {
            let repo_id: String = row.get(0)?;

            // Create repository
            let repository = Repository {
                id: Some(repo_id.clone()),
                namespace: row.get(1)?,
                namespace_alias: row.get(2)?,
                repo_path: row.get(3)?,
                git_path: row.get(4)?,
                user_id: row.get(5)?,
                name: row.get(6)?,
                email: row.get(7)?,
                client_id: row.get(8)?,
                client_name: row.get(9)?,
                client_contact_person: row.get(10)?,
                client_address: row.get(11)?,
                project_number: row.get(12)?,
                service: row.get(13)?,
                service_username: row.get(14)?,
                git_log_dates: None,
                timesheet: None,
            };

            Ok((repo_id, repository))
        })?;

        let mut repositories = Vec::new();
        for repo_result in repo_rows {
            let (repo_id, mut repo) = repo_result?;

            // Load git log dates for this repository (HashMap<i32, HashMap<u32, HashSet<u32>>>)
            let mut git_log_dates = std::collections::HashMap::new();

            // Get all years for this repository
            let mut year_stmt =
                conn.prepare("SELECT year FROM git_log_years WHERE repository_id = ?1")?;

            let year_rows = year_stmt.query_map(params![repo_id], |row| {
                let year: i32 = row.get(0)?;
                Ok(year)
            })?;

            for year_result in year_rows {
                let year = year_result?;
                let mut months_map = std::collections::HashMap::new();

                // Get all months for this year
                let mut month_stmt = conn.prepare(
                    "SELECT month FROM git_log_months
                     WHERE repository_id = ?1 AND year = ?2",
                )?;

                let month_rows = month_stmt.query_map(params![repo_id, year], |row| {
                    let month: u32 = row.get(0)?;
                    Ok(month)
                })?;

                for month_result in month_rows {
                    let month = month_result?;
                    let mut days_set = std::collections::HashSet::new();

                    // Get all days for this month
                    let mut day_stmt = conn.prepare(
                        "SELECT day FROM git_log_days
                         WHERE repository_id = ?1 AND year = ?2 AND month = ?3",
                    )?;

                    let day_rows = day_stmt.query_map(params![repo_id, year, month], |row| {
                        let day: u32 = row.get(0)?;
                        Ok(day)
                    })?;

                    for day_result in day_rows {
                        days_set.insert(day_result?);
                    }

                    if !days_set.is_empty() {
                        months_map.insert(month, days_set);
                    }
                }

                if !months_map.is_empty() {
                    git_log_dates.insert(year, months_map);
                }
            }

            repo.git_log_dates = if git_log_dates.is_empty() {
                None
            } else {
                Some(git_log_dates)
            };

            // Here we would also load timesheet data if needed
            // but that's omitted for brevity

            // Add to repositories list
            repositories.push(repo);
        }

        // Create client repository
        let client_repository = ClientRepositories {
            client: Some(client),
            user,
            repositories: if repositories.is_empty() {
                None
            } else {
                Some(repositories)
            },
            requires_approval,
            approver,
        };

        result.push(client_repository);
    }

    Ok(result)
}

pub fn get_canonical_path(path: &str) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|err| {
        println!("Canonicalization of repo path failed: {}", err);
        std::process::exit(exitcode::CANTCREAT);
    });
    path.to_str().map(|x| x.to_string()).unwrap()
}
