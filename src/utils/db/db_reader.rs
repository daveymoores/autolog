use crate::data::client_repositories::ClientRepositories;
use crate::data::repository::Repository;
use crate::interface::help_prompt::ConfigurationDoc;
use crate::utils::is_test_mode;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct ClientRepository {
    pub client_id: String,
}

const DB_FILE_NAME: &str = "autolog.db";

/// Directly load configuration document from database
pub fn load_config_doc_from_db() -> Result<ConfigurationDoc, Box<dyn std::error::Error>> {
    let conn = get_connection()?;
    load_config_doc(&conn)
}

/// Directly save configuration document to database
pub fn save_config_doc_to_db(
    config_doc: &ConfigurationDoc,
) -> Result<(), Box<dyn std::error::Error>> {
    if is_test_mode() {
        return Ok(());
    }

    let mut conn = get_connection()?;
    let tx = conn.transaction()?;

    // Try to write each client repository to the database
    let result = (|| {
        // First, remove any client repositories that are no longer in the config_doc
        remove_deleted_client_repositories(&tx, config_doc)?;

        for client_repo in config_doc.iter() {
            save_client_repository(&tx, client_repo)?;
        }
        Ok(())
    })();

    // Only commit if everything succeeded
    if result.is_ok() {
        tx.commit()?;
    } else {
        let _ = tx.rollback();
        println!("Changes rolled back due to error");
        return result;
    }

    Ok(())
}

fn remove_deleted_client_repositories(
    tx: &Transaction,
    config_doc: &ConfigurationDoc,
) -> Result<(), Box<dyn std::error::Error>> {
    // Fetch all existing clients from the database
    let existing_client_ids = fetch_all_clients(tx)?;

    // Iterate over the existing client_ids and remove those not in config_doc
    for client_id in existing_client_ids {
        if !config_doc.iter().any(|client_repo| {
            client_repo
                .client
                .as_ref()
                .map_or(false, |client| client.id == client_id)
        }) {
            delete_client_repositories_by_client_id(tx, &client_id)?;
        }
    }

    Ok(())
}

fn fetch_all_clients(tx: &Transaction) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut stmt = tx.prepare("SELECT id FROM clients")?;
    let mut client_id_vec: Vec<String> = Vec::new();

    let client_repo_iter = stmt.query_map(params![], |row| row.get(0))?;

    for client_repo in client_repo_iter {
        client_id_vec.push(client_repo?);
    }

    Ok(client_id_vec)
}

fn delete_client_repositories_by_client_id(
    tx: &Transaction,
    client_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // First fetch all repositories for this client
    let mut stmt = tx.prepare("SELECT id FROM repositories WHERE client_id = ?")?;
    let repo_ids: Vec<String> = stmt
        .query_map(params![client_id], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;

    // Delete data for each repository
    for repo_id in &repo_ids {
        // Delete timesheet entries
        tx.execute(
            "DELETE FROM timesheet_entries WHERE repository_id = ?",
            params![repo_id],
        )?;

        // Delete git log days
        tx.execute(
            "DELETE FROM git_log_days WHERE repository_id = ?",
            params![repo_id],
        )?;

        // Delete git log months
        tx.execute(
            "DELETE FROM git_log_months WHERE repository_id = ?",
            params![repo_id],
        )?;

        // Delete git log years
        tx.execute(
            "DELETE FROM git_log_years WHERE repository_id = ?",
            params![repo_id],
        )?;
    }

    // Delete repositories
    tx.execute(
        "DELETE FROM repositories WHERE client_id = ?",
        params![client_id],
    )?;

    // Delete approvers (need to find client_repository_ids first)
    let mut stmt = tx.prepare("SELECT id FROM client_repositories WHERE client_id = ?")?;
    let client_repo_ids: Vec<i64> = stmt
        .query_map(params![client_id], |row| row.get(0))?
        .collect::<Result<Vec<i64>, _>>()?;

    for client_repo_id in client_repo_ids {
        tx.execute(
            "DELETE FROM approvers WHERE client_repository_id = ?",
            params![client_repo_id],
        )?;
    }

    // Delete from client_repositories
    tx.execute(
        "DELETE FROM client_repositories WHERE client_id = ?",
        params![client_id],
    )?;

    // Finally delete the client itself
    tx.execute("DELETE FROM clients WHERE id = ?", params![client_id])?;

    println!("Deleted client {} and all associated data", client_id);

    Ok(())
}

/// Get the platform-specific path for the database
pub fn get_db_path() -> PathBuf {
    if is_test_mode() {
        return PathBuf::from("file:memdb_test?mode=memory&cache=shared");
    }

    // Get Homebrew prefix - this should always be available when installed through Homebrew
    let homebrew_prefix = std::env::var("HOMEBREW_PREFIX")
        .expect("HOMEBREW_PREFIX environment variable not found. This application should be installed via Homebrew.");

    // Use Homebrew's etc directory pattern: #{HOMEBREW_PREFIX}/etc/#{name}
    let config_dir = PathBuf::from(homebrew_prefix).join("etc").join("autolog");

    // Ensure directory exists
    fs::create_dir_all(&config_dir).expect(&format!(
        "Failed to create config directory at {:?}",
        config_dir
    ));

    config_dir.join(DB_FILE_NAME)
}

/// Get a database connection
pub fn get_connection() -> Result<Connection> {
    let db_path = get_db_path();
    let db_path_str = db_path.to_str().unwrap_or("");

    let conn = if is_test_mode() {
        // Use URI flags to properly open the shared in-memory database
        Connection::open_with_flags(
            db_path_str,
            rusqlite::OpenFlags::SQLITE_OPEN_URI
                | rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
        )
        .context(format!(
            "Failed to open in-memory database with URI {}",
            db_path_str
        ))?
    } else {
        Connection::open(&db_path).context(format!("Failed to open database at {:?}", db_path))?
    };

    // Enable foreign key support
    conn.execute("PRAGMA foreign_keys = ON", [])?;

    // Initialize schema if needed
    init_schema(&conn).context("Failed to initialize database schema")?;

    Ok(conn)
}

pub fn should_check_for_updates() -> Result<bool, Box<dyn std::error::Error>> {
    let conn = get_connection()?;

    // Get the most recent cache entry
    let result = conn.query_row(
        "SELECT last_checked, latest_version FROM version_cache
         ORDER BY last_checked DESC LIMIT 1",
        [],
        |row| {
            let last_checked: String = row.get(0)?;
            let latest_version: String = row.get(1)?;
            Ok((last_checked, latest_version))
        },
    );

    match result {
        Ok((last_checked_str, _)) => {
            // Parse the last_checked timestamp
            let last_checked = chrono::DateTime::parse_from_rfc3339(&last_checked_str)
                .map_err(|e| format!("Invalid timestamp format: {}", e))?
                .with_timezone(&chrono::Utc);

            // Get current time
            let now = chrono::Utc::now();
            // Check once a day (86400 seconds)
            Ok((now - last_checked).num_seconds() >= 86400)
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            // No cache entry yet, should check
            Ok(true)
        }
        Err(e) => Err(Box::new(e)),
    }
}

/// Update the version cache with the latest version
pub fn update_version_cache(latest_version: &str) -> Result<(), Box<dyn std::error::Error>> {
    let conn = get_connection()?;
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO version_cache (last_checked, latest_version) VALUES (?, ?)",
        params![now, latest_version],
    )?;

    // Keep only the most recent 5 entries to prevent unlimited growth
    conn.execute(
        "DELETE FROM version_cache WHERE id NOT IN (
            SELECT id FROM version_cache ORDER BY last_checked DESC LIMIT 5
        )",
        [],
    )?;

    Ok(())
}

/// Get the latest version from cache without checking for updates
pub fn get_cached_version() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let conn = get_connection()?;

    match conn.query_row(
        "SELECT latest_version FROM version_cache
         ORDER BY last_checked DESC LIMIT 1",
        [],
        |row| row.get::<_, String>(0),
    ) {
        Ok(version) => Ok(Some(version)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(Box::new(e)),
    }
}

/// Initialize the database schema if tables don't exist
fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS version_cache (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        last_checked TEXT NOT NULL,
        latest_version TEXT NOT NULL
        )",
        [],
    )
    .context("Failed to create version_cache table")?;

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
    default_hours FLOAT NOT NULL DEFAULT 8.0,
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
    )
    .context("Failed to create git_log_days table")?;

    // Create simplified timesheet table with direct column storage
    conn.execute(
        "CREATE TABLE IF NOT EXISTS timesheet_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repository_id TEXT NOT NULL,
    year TEXT NOT NULL,
    month TEXT NOT NULL,
    day INTEGER NOT NULL,
    hours REAL,
    weekend INTEGER NOT NULL DEFAULT 0,
    user_edited INTEGER NOT NULL DEFAULT 0,
    extra_data TEXT, -- JSON for any additional fields
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(repository_id, year, month, day),
    FOREIGN KEY (repository_id) REFERENCES repositories (id)
    )",
        [],
    )
    .context("Failed to create timesheet_entries table")?;

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
    FOREIGN KEY (client_repository_id) REFERENCES client_repositories (id),
    UNIQUE(client_repository_id)
    )",
        [],
    )
    .context("Failed to create approvers table")?;

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

/// Save a ClientRepositories object to the database, including handling repository removals
pub fn save_client_repository(
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

        // IMPORTANT: Get all existing repositories for this client
        let mut existing_repos = Vec::new();
        let mut stmt = tx.prepare("SELECT id, namespace FROM repositories WHERE client_id = ?")?;
        let rows = stmt.query_map(params![client.id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;

        for row in rows {
            if let Ok((id, namespace)) = row {
                existing_repos.push((id, namespace));
            }
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
                // Check if approver already exists for this client_repository
                let approver_exists: bool = tx
                    .query_row(
                        "SELECT 1 FROM approvers WHERE client_repository_id = ?1 LIMIT 1",
                        params![client_repo_id],
                        |_| Ok(true),
                    )
                    .unwrap_or(false);

                if approver_exists {
                    // Update existing approver
                    tx.execute(
                        "UPDATE approvers SET approvers_name = ?1, approvers_email = ?2
                         WHERE client_repository_id = ?3",
                        params![
                            approver.approvers_name,
                            approver.approvers_email,
                            client_repo_id
                        ],
                    )?;
                } else {
                    // Insert new approver
                    tx.execute(
                        "INSERT INTO approvers (client_repository_id, approvers_name, approvers_email)
                         VALUES (?1, ?2, ?3)",
                        params![
                            client_repo_id,
                            approver.approvers_name,
                            approver.approvers_email
                        ],
                    )?;
                }
            }
        }

        // Track repository IDs that still exist in the ClientRepositories
        let mut current_repo_ids = Vec::new();

        // Save repositories if they exist
        if let Some(repositories) = &client_repo.repositories {
            for repo in repositories {
                if let Some(id) = &repo.id {
                    current_repo_ids.push(id.clone());

                    // Verify repository exists before proceeding
                    let repo_exists: bool = tx
                        .query_row(
                            "SELECT 1 FROM repositories WHERE id = ?1 LIMIT 1",
                            params![id],
                            |_| Ok(true),
                        )
                        .unwrap_or(false);

                    if !repo_exists {
                        // Insert repository
                        tx.execute(
                            "INSERT INTO repositories (id, name) VALUES (?1, ?2)",
                            params![id, repo.name.as_ref().unwrap_or(&String::from("Unknown"))],
                        )?;
                    }

                    // Save repository
                    tx.execute(
                        "INSERT OR REPLACE INTO repositories (
                        id, namespace, namespace_alias, repo_path, git_path,
                        user_id, name, email, client_id, client_name,
                        client_contact_person, client_address, project_number,
                        service, service_username, default_hours
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
                            repo.service_username,
                            repo.default_hours
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
                    } else {
                        println!("No git log dates for repo {}", id);
                    }

                    // Save timesheet if present
                    if let Some(timesheet) = &repo.timesheet {
                        // Prepare statement for efficiency
                        let mut stmt = tx.prepare(
                            "INSERT OR REPLACE INTO timesheet_entries (
                            repository_id, year, month, day, hours, weekend, user_edited, extra_data, updated_at
                            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, CURRENT_TIMESTAMP)",
                        )?;

                        for (year, months) in timesheet {
                            for (month, days) in months {
                                for (day_index, day_data) in days.iter().enumerate() {
                                    let day = day_index + 1; // Convert 0-based index to 1-based day

                                    // Extract common fields
                                    let hours = day_data
                                        .get("hours")
                                        .and_then(|v| v.as_f64())
                                        .unwrap_or(0.0);

                                    let weekend = day_data
                                        .get("weekend")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);

                                    let user_edited = day_data
                                        .get("user_edited")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);

                                    // Extract any additional fields as JSON
                                    let mut extra_data = Map::new();
                                    for (key, value) in day_data.iter() {
                                        if key != "hours"
                                            && key != "weekend"
                                            && key != "user_edited"
                                        {
                                            extra_data.insert(key.clone(), value.clone());
                                        }
                                    }

                                    let extra_json = if extra_data.is_empty() {
                                        None
                                    } else {
                                        Some(serde_json::to_string(&extra_data)?)
                                    };

                                    // Insert or update the entry
                                    stmt.execute(params![
                                        id,
                                        year,
                                        month,
                                        day,
                                        hours,
                                        weekend as i32,
                                        user_edited as i32,
                                        extra_json
                                    ])?;
                                }
                            }
                        }
                    } else {
                        println!("No timesheet for repo {}", id);
                    }
                }
            }
        }

        // IMPORTANT: Delete repositories that no longer exist in the client's repository list
        for (repo_id, _namespace) in existing_repos {
            if !current_repo_ids.contains(&repo_id) {
                // Delete timesheet entries for this repository
                tx.execute(
                    "DELETE FROM timesheet_entries WHERE repository_id = ?",
                    params![repo_id],
                )?;

                // Delete git log days
                tx.execute(
                    "DELETE FROM git_log_days WHERE repository_id = ?",
                    params![repo_id],
                )?;

                // Delete git log months
                tx.execute(
                    "DELETE FROM git_log_months WHERE repository_id = ?",
                    params![repo_id],
                )?;

                // Delete git log years
                tx.execute(
                    "DELETE FROM git_log_years WHERE repository_id = ?",
                    params![repo_id],
                )?;

                // Finally delete the repository itself
                tx.execute("DELETE FROM repositories WHERE id = ?", params![repo_id])?;
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
      service, service_username, default_hours
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
                default_hours: row.get(15)?,
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

            // Load timesheet data using the new simplified structure
            let mut timesheet_years = HashMap::new();

            // Get all timesheet entries for this repository
            let mut timesheet_stmt = conn.prepare(
                "SELECT year, month, day, hours, weekend, user_edited, extra_data
         FROM timesheet_entries
         WHERE repository_id = ?1
         ORDER BY year, month, day",
            )?;

            let timesheet_rows = timesheet_stmt.query_map(params![repo_id], |row| {
                let year: String = row.get(0)?;
                let month: String = row.get(1)?;
                let day: i32 = row.get(2)?;
                let hours: f64 = row.get(3)?;
                let weekend: bool = row.get::<_, i32>(4)? != 0;
                let user_edited: bool = row.get::<_, i32>(5)? != 0;
                let extra_data: Option<String> = row.get(6)?;

                Ok((year, month, day, hours, weekend, user_edited, extra_data))
            })?;

            for entry_result in timesheet_rows {
                let (year, month, day, hours, weekend, user_edited, extra_data) = entry_result?;

                // Create a Map to store the day data
                let mut day_data = Map::new();

                // Add common fields
                day_data.insert(
                    "hours".to_string(),
                    Value::Number(Number::from_f64(hours).unwrap_or(Number::from(0))),
                );
                day_data.insert("weekend".to_string(), Value::Bool(weekend));
                day_data.insert("user_edited".to_string(), Value::Bool(user_edited));

                // Add any extra data fields
                if let Some(extra_json) = extra_data {
                    if let Ok(extra_map) = serde_json::from_str::<Map<String, Value>>(&extra_json) {
                        for (key, value) in extra_map {
                            day_data.insert(key, value);
                        }
                    }
                }

                // Get or create the year map
                let year_map = timesheet_years.entry(year).or_insert_with(HashMap::new);

                // Get or create the month vector
                let month_days = year_map
                    .entry(month)
                    .or_insert_with(|| Vec::with_capacity(31));

                // Ensure the month vector has enough capacity
                while month_days.len() < day as usize {
                    month_days.push(Map::new());
                }

                // Insert the day data at the appropriate index (day - 1)
                month_days[day as usize - 1] = day_data;
            }

            repo.timesheet = if timesheet_years.is_empty() {
                None
            } else {
                Some(timesheet_years)
            };

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

#[cfg(test)]
pub mod test_utils {
    use super::*;
    use crate::data::client_repositories::{Approver, Client, ClientRepositories, User};
    use crate::data::repository::Repository;
    use rusqlite::Connection;
    use std::collections::{HashMap, HashSet};

    // Helper function to set up a test database
    pub fn setup_test_db() -> Connection {
        // Use in-memory database for tests
        let conn = Connection::open("file:memdb_test?mode=memory&cache=shared").unwrap();
        init_schema(&conn).unwrap();

        // Clear all tables
        conn.execute("DELETE FROM approvers", []).unwrap();
        conn.execute("DELETE FROM client_repositories", []).unwrap();
        conn.execute("DELETE FROM timesheet_entries", []).unwrap();
        conn.execute("DELETE FROM git_log_days", []).unwrap();
        conn.execute("DELETE FROM git_log_months", []).unwrap();
        conn.execute("DELETE FROM git_log_years", []).unwrap();
        conn.execute("DELETE FROM repositories", []).unwrap();
        conn.execute("DELETE FROM users", []).unwrap();
        conn.execute("DELETE FROM clients", []).unwrap();
        conn.execute("DELETE FROM version_cache", []).unwrap();

        conn
    }

    // Helper to create a basic client repository
    pub fn create_test_client(client_name: &str, repository_name: &str) -> ClientRepositories {
        let client = Client {
            id: client_name.to_lowercase().to_string(),
            client_name: client_name.to_string(),
            client_address: "123 Test St".to_string(),
            client_contact_person: "Test Contact".to_string(),
        };

        let user = User {
            id: format!("user-{}", client_name),
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
            is_alias: false,
            thumbnail: None,
        };

        let approver = Approver {
            approvers_name: Some("Approver Name".to_string()),
            approvers_email: Some("approver@example.com".to_string()),
        };

        let repository = Repository {
            id: Some(format!("repo-{}", repository_name.to_lowercase())),
            namespace: Some(repository_name.to_string()),
            namespace_alias: None,
            repo_path: Some("/test/path".to_string()),
            git_path: Some("/test/git/path".to_string()),
            user_id: Some(user.id.clone()),
            name: Some("Test Repo".to_string()),
            email: Some("repo@example.com".to_string()),
            client_id: Some(client_name.to_string()),
            client_name: Some(client_name.to_string()),
            client_contact_person: Some("Test Contact".to_string()),
            client_address: Some("123 Test St".to_string()),
            project_number: Some("PROJECT-123".to_string()),
            service: Some("GitHub".to_string()),
            service_username: Some("testuser".to_string()),
            git_log_dates: Some(create_test_git_log_dates()),
            timesheet: Some(create_test_timesheet()),
            default_hours: Some(8.0),
        };

        ClientRepositories {
            client: Some(client),
            user: Some(user),
            repositories: Some(vec![repository]),
            requires_approval: Some(true),
            approver: Some(approver),
        }
    }

    // Helper to create test git log dates
    pub fn create_test_git_log_dates() -> HashMap<i32, HashMap<u32, HashSet<u32>>> {
        let mut git_log_dates = HashMap::new();
        // Create data for May 2023
        let mut months_2023 = HashMap::new();
        let mut days_may = HashSet::new();
        days_may.insert(15);
        days_may.insert(16);
        months_2023.insert(5, days_may);
        git_log_dates.insert(2023, months_2023);

        // Create data for November 2021
        let mut months_2021 = HashMap::new();
        let mut days_nov = HashSet::new();
        days_nov.insert(1); // 1st of November
        months_2021.insert(11, days_nov);
        git_log_dates.insert(2021, months_2021);

        git_log_dates
    }

    // Helper to create test timesheet
    pub fn create_test_timesheet()
    -> HashMap<String, HashMap<String, Vec<serde_json::Map<String, serde_json::Value>>>> {
        let mut timesheet = HashMap::new();

        // Create data for May 2023
        let mut may_2023 = HashMap::new();
        let mut days_may = Vec::new();
        for _ in 0..30 {
            days_may.push(serde_json::Map::new());
        }
        let mut day_data_may = serde_json::Map::new();
        day_data_may.insert(
            "hours".to_string(),
            serde_json::Value::Number(serde_json::Number::from(8)),
        );
        day_data_may.insert("weekend".to_string(), serde_json::Value::Bool(false));
        day_data_may.insert("user_edited".to_string(), serde_json::Value::Bool(true));
        days_may[14] = day_data_may;
        may_2023.insert("05".to_string(), days_may);
        timesheet.insert("2023".to_string(), may_2023);

        // Create data for November 2021
        let mut nov_2021 = HashMap::new();
        let mut days_nov = Vec::new();
        for _ in 0..30 {
            days_nov.push(serde_json::Map::new());
        }
        let mut day_data_nov = serde_json::Map::new();
        day_data_nov.insert(
            "hours".to_string(),
            serde_json::Value::Number(serde_json::Number::from(8)),
        );
        day_data_nov.insert("weekend".to_string(), serde_json::Value::Bool(false));
        day_data_nov.insert("user_edited".to_string(), serde_json::Value::Bool(true));
        days_nov[0] = day_data_nov; // 1st of November
        nov_2021.insert("11".to_string(), days_nov);
        timesheet.insert("2021".to_string(), nov_2021);

        timesheet
    }

    // Helper to count entities in database
    pub fn count_entities(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| {
            row.get(0)
        })
        .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::test_utils::*;
    use super::*;
    use rusqlite::Connection;
    use std::env;
    use tempfile::tempdir;

    #[test]
    #[serial_test::serial]
    fn test_save_and_delete_single_client() {
        // Force test mode
        unsafe { env::set_var("TEST_MODE", "1") };

        let mut conn = setup_test_db();
        let tx = conn.transaction().unwrap();

        // Create configuration with one client
        let client_repo = create_test_client("client1", "repo1");
        let config_doc = vec![client_repo];

        // Save the client
        save_client_repository(&tx, &config_doc[0]).unwrap();
        tx.commit().unwrap();

        // Verify data was saved
        assert_eq!(count_entities(&conn, "clients"), 1);
        assert_eq!(count_entities(&conn, "repositories"), 1);
        assert_eq!(count_entities(&conn, "users"), 1);
        assert_eq!(count_entities(&conn, "client_repositories"), 1);
        assert_eq!(count_entities(&conn, "approvers"), 1);
        assert_eq!(count_entities(&conn, "git_log_years"), 2);
        assert_eq!(count_entities(&conn, "git_log_months"), 2);
        assert_eq!(count_entities(&conn, "git_log_days"), 3); // three days
        assert_eq!(count_entities(&conn, "timesheet_entries"), 60); // two months so 60 days

        // Now delete the client
        let tx = conn.transaction().unwrap();
        delete_client_repositories_by_client_id(&tx, "client1").unwrap();
        tx.commit().unwrap();

        // Verify all data was deleted
        assert_eq!(count_entities(&conn, "clients"), 0);
        assert_eq!(count_entities(&conn, "repositories"), 0);
        assert_eq!(count_entities(&conn, "client_repositories"), 0);
        assert_eq!(count_entities(&conn, "approvers"), 0);
        assert_eq!(count_entities(&conn, "git_log_years"), 0);
        assert_eq!(count_entities(&conn, "git_log_months"), 0);
        assert_eq!(count_entities(&conn, "git_log_days"), 0);
        assert_eq!(count_entities(&conn, "timesheet_entries"), 0);

        // User should still exist as it's not directly tied to client deletion
        assert_eq!(count_entities(&conn, "users"), 1);
    }

    #[test]
    #[serial_test::serial]
    fn test_save_and_remove_deleted_clients() {
        // Force test mode
        unsafe { env::set_var("TEST_MODE", "1") };

        let mut conn = setup_test_db();

        // Create configuration with two clients
        let client_repo1 = create_test_client("client1", "repo1");
        let client_repo2 = create_test_client("client2", "repo2");
        let config_doc = vec![client_repo1, client_repo2];

        // Save both clients
        let tx = conn.transaction().unwrap();
        for client_repo in &config_doc {
            save_client_repository(&tx, client_repo).unwrap();
        }
        tx.commit().unwrap();

        // Verify both clients were saved
        assert_eq!(count_entities(&conn, "clients"), 2);
        assert_eq!(count_entities(&conn, "repositories"), 2);

        // Now create a new config with only the first client
        let updated_config_doc = vec![config_doc[0].clone()];

        // Use the remove_deleted_client_repositories function
        let tx = conn.transaction().unwrap();
        remove_deleted_client_repositories(&tx, &updated_config_doc).unwrap();
        tx.commit().unwrap();

        // Verify only one client remains
        assert_eq!(count_entities(&conn, "clients"), 1);
        assert_eq!(count_entities(&conn, "repositories"), 1);

        // Verify the remaining client is client1
        let client_id: String = conn
            .query_row("SELECT id FROM clients", [], |row| row.get(0))
            .unwrap();
        assert_eq!(client_id, "client1");
    }

    #[test]
    #[serial_test::serial]
    fn test_full_save_config_doc_with_removal() {
        // Force test mode
        unsafe { env::set_var("TEST_MODE", "1") };

        // Set a custom DB path for testing
        let temp_dir = tempdir().unwrap();
        let test_db_path = temp_dir.path().join("test.db");

        // Override the get_db_path function for this test
        // Note: This requires modifying get_db_path to be mockable or using a crate like mockall
        // For this example, we'll use a simple approach to isolate our tests

        // Create and populate the database
        let mut conn = Connection::open(&test_db_path).unwrap();
        init_schema(&conn).unwrap();

        // Create two clients
        let client_repo1 = create_test_client("client1", "repo1");
        let client_repo2 = create_test_client("client2", "repo2");
        let config_doc = vec![client_repo1.clone(), client_repo2.clone()];

        // Save both clients manually to the test database
        {
            let tx = conn.transaction().unwrap();
            for client_repo in &config_doc {
                save_client_repository(&tx, client_repo).unwrap();
            }
            tx.commit().unwrap();
        }

        // Verify both clients were saved
        assert_eq!(count_entities(&conn, "clients"), 2);
        assert_eq!(count_entities(&conn, "repositories"), 2);

        // Now create an updated config that only includes client1
        let updated_config_doc = vec![client_repo1];

        // Need to manually execute the save_config_doc_to_db logic since we're using a custom DB
        {
            let tx = conn.transaction().unwrap();
            remove_deleted_client_repositories(&tx, &updated_config_doc).unwrap();

            for client_repo in updated_config_doc.iter() {
                save_client_repository(&tx, client_repo).unwrap();
            }
            tx.commit().unwrap();
        }

        // Verify only one client remains
        assert_eq!(count_entities(&conn, "clients"), 1);
        assert_eq!(count_entities(&conn, "repositories"), 1);

        // Check that it's client1
        let client_id: String = conn
            .query_row("SELECT id FROM clients", [], |row| row.get(0))
            .unwrap();
        assert_eq!(client_id, "client1");
    }

    #[test]
    #[serial_test::serial]
    fn test_version_cache_functions() {
        // Setup
        unsafe { env::set_var("TEST_MODE", "1") };
        let conn = setup_test_db();

        // No cache exists initially
        assert_eq!(should_check_for_updates().expect("Should succeed"), true);
        assert_eq!(get_cached_version().expect("Should succeed"), None);

        // Insert old timestamp to test time-based check
        let timestamp = (chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339();
        conn.execute(
            "INSERT INTO version_cache (last_checked, latest_version) VALUES (?, ?)",
            params![timestamp, "0.9.0"],
        )
        .unwrap();

        // Old cache entry should trigger check
        assert!(should_check_for_updates().unwrap());

        // Update cache with recent version
        update_version_cache("1.0.0").expect("Should succeed");
        assert_eq!(
            get_cached_version().expect("Should succeed"),
            Some("1.0.0".to_string())
        );

        // Recent cache entry should not trigger check
        assert_eq!(should_check_for_updates().expect("Should succeed"), false);

        // Test most recent version is returned
        let newer_timestamp = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO version_cache (last_checked, latest_version) VALUES (?, ?)",
            params![newer_timestamp, "1.1.0"],
        )
        .unwrap();

        assert_eq!(get_cached_version().unwrap(), Some("1.1.0".to_string()));

        // Test only 5 most recent entries are kept
        for i in 2..8 {
            update_version_cache(&format!("1.{}.0", i)).unwrap();
        }

        assert_eq!(count_entities(&conn, "version_cache"), 5);

        // Verify correct versions retained
        let versions: Vec<String> = conn
            .prepare("SELECT latest_version FROM version_cache ORDER BY last_checked ASC")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(versions.len(), 5);
        assert!(versions.contains(&"1.3.0".to_string()));
        assert!(versions.contains(&"1.7.0".to_string()));
        assert!(!versions.contains(&"1.1.0".to_string()));
        assert!(!versions.contains(&"1.2.0".to_string()));
    }
}
