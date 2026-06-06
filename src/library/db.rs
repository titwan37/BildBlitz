// src/library/db.rs

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;
use std::path::PathBuf;
use crate::engine::gallery::FileInfo;

#[derive(Clone)]
pub struct DatabaseManager {
    pool: SqlitePool,
}

impl DatabaseManager {
    pub fn get_pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn new() -> Result<Self> {
        let data_dir = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("Unable to locate data directory"))?
            .join("BildBlitz");
        std::fs::create_dir_all(&data_dir)?;
        let db_path = data_dir.join("bildblitz.db");
        
        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true);
            
        let pool = SqlitePool::connect_with(options).await?;
        let manager = DatabaseManager { pool };
        manager.initialize_schema().await?;
        Ok(manager)
    }

    async fn initialize_schema(&self) -> Result<()> {
        sqlx::query(
            "
            CREATE TABLE IF NOT EXISTS images (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                size INTEGER,
                modified INTEGER,
                exif_json TEXT,
                phash TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_phash ON images(phash);
            
            CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE
            );
            
            CREATE TABLE IF NOT EXISTS image_tags (
                image_id INTEGER NOT NULL,
                tag_id INTEGER NOT NULL,
                FOREIGN KEY(image_id) REFERENCES images(id),
                FOREIGN KEY(tag_id) REFERENCES tags(id),
                UNIQUE(image_id, tag_id)
            );
            
            CREATE TABLE IF NOT EXISTS virtual_collections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            
            CREATE TABLE IF NOT EXISTS collection_members (
                collection_id INTEGER NOT NULL,
                image_id INTEGER NOT NULL,
                FOREIGN KEY(collection_id) REFERENCES virtual_collections(id),
                FOREIGN KEY(image_id) REFERENCES images(id),
                UNIQUE(collection_id, image_id)
            );
            "
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_image_metadata(&self, file: FileInfo, phash: Option<String>) -> Result<()> {
        let modified_ts = file.modified
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
            
        sqlx::query(
            "INSERT OR REPLACE INTO images (path, size, modified, exif_json, phash) VALUES (?1, ?2, ?3, ?4, ?5)"
        )
        .bind(file.path.to_string_lossy().to_string())
        .bind(file.size as i64)
        .bind(modified_ts)
        .bind("{}")
        .bind(phash)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    pub async fn query_by_hash(&self, hash: &str) -> Result<Option<i64>> {
        let row: Option<(i64,)> = sqlx::query_as("SELECT id FROM images WHERE phash = ?1")
            .bind(hash)
            .fetch_optional(&self.pool)
            .await?;
            
        Ok(row.map(|r| r.0))
    }

    pub async fn store_virtual_collection(&self, name: &str, image_paths: &[PathBuf]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        
        let collection_id: i64 = sqlx::query(
            "INSERT INTO virtual_collections (name, created_at) VALUES (?1, strftime('%s','now')) RETURNING id"
        )
        .bind(name)
        .fetch_one(&mut *tx)
        .await?
        .get(0);
        
        for path in image_paths {
            let path_str = path.to_string_lossy().to_string();
            let image_id: i64 = sqlx::query("SELECT id FROM images WHERE path = ?1")
                .bind(path_str)
                .fetch_one(&mut *tx)
                .await?
                .get(0);
                
            sqlx::query("INSERT INTO collection_members (collection_id, image_id) VALUES (?1, ?2)")
                .bind(collection_id)
                .bind(image_id)
                .execute(&mut *tx)
                .await?;
        }
        
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_duplicates(&self) -> Result<Vec<Vec<PathBuf>>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT path, phash FROM images WHERE phash IS NOT NULL ORDER BY phash"
        )
        .fetch_all(&self.pool)
        .await?;

        let mut groups: std::collections::HashMap<String, Vec<PathBuf>> = std::collections::HashMap::new();
        for (path, phash) in rows {
            groups.entry(phash).or_default().push(PathBuf::from(path));
        }

        let duplicates: Vec<Vec<PathBuf>> = groups.into_values()
            .filter(|group| group.len() > 1)
            .collect();
        
        Ok(duplicates)
    }
}
