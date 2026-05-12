// Data layer — SQLite database initialization and migration
//
// Uses sqlx with SQLite backend. Gracefully falls back to in-memory-only
// mode when the database URL is empty or connection fails.

use sqlx::SqlitePool;

/// Initialize the database pool (if configured) and run migrations.
/// Returns None if no database is configured or connection fails —
/// the system will fall back to in-memory-only mode.
pub async fn init_db(config: &crate::config::DatabaseConfig) -> Option<SqlitePool> {
    if config.url.is_empty() {
        tracing::warn!("Database URL is empty — running in memory-only mode");
        return None;
    }

    match SqlitePool::connect(&config.url).await {
        Ok(pool) => {
            // Set WAL mode for better concurrent read performance
            let _ = sqlx::query("PRAGMA journal_mode = WAL").execute(&pool).await;
            let _ = sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await;

            tracing::info!("SQLite database connected: {}", config.url);
            if let Err(e) = run_migrations(&pool).await {
                tracing::warn!("Migration warning (non-fatal): {e}");
            }
            Some(pool)
        }
        Err(e) => {
            tracing::warn!("Database connection failed: {e} — running in memory-only mode");
            None
        }
    }
}

/// Run schema migrations from the embedded SQL file.
/// Uses include_str! so the migration SQL is baked into the binary at compile time.
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let sql = include_str!("../../migrations/0001_init.sql");

    for statement in sql.split(';') {
        // Strip `--` comment lines, then trim
        let clean: String = statement
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty() && !trimmed.starts_with("--")
            })
            .collect::<Vec<_>>()
            .join("\n");

        let trimmed = clean.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Skip PRAGMA lines (already handled in init_db)
        if trimmed.to_uppercase().starts_with("PRAGMA") {
            continue;
        }

        if let Err(e) = sqlx::query(trimmed).execute(pool).await {
            // "already exists" and certain non-fatal errors
            let msg = e.to_string();
            if msg.contains("already exists") || msg.contains("duplicate column") {
                continue;
            }
            tracing::warn!("Migration statement warning: {e}");
        }
    }

    tracing::info!("Database migrations applied successfully");
    Ok(())
}
