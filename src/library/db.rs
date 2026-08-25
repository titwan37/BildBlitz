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
                phash TEXT,
                sketch_score REAL,
                binary_score REAL,
                raytrace_score REAL,
                lab_l REAL,
                lab_a REAL,
                lab_b REAL,
                aspect_ratio REAL,
                palette_json TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_phash ON images(phash);
            CREATE INDEX IF NOT EXISTS idx_path_mod ON images(path, modified);
            
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

        // Safely migrate existing databases if columns are missing
        let _ = sqlx::query("ALTER TABLE images ADD COLUMN lab_l REAL").execute(&self.pool).await;
        let _ = sqlx::query("ALTER TABLE images ADD COLUMN lab_a REAL").execute(&self.pool).await;
        let _ = sqlx::query("ALTER TABLE images ADD COLUMN lab_b REAL").execute(&self.pool).await;
        let _ = sqlx::query("ALTER TABLE images ADD COLUMN aspect_ratio REAL").execute(&self.pool).await;
        let _ = sqlx::query("ALTER TABLE images ADD COLUMN palette_json TEXT").execute(&self.pool).await;

        Ok(())
    }

    pub async fn get_cached_feature(&self, path: &std::path::Path, modified_ts: i64) -> Result<Option<(crate::engine::auto_group::ImageFeature, String)>> {
        let path_str = path.to_string_lossy().to_string();
        let row: Option<(
            Option<String>, // phash
            Option<f64>,    // sketch_score
            Option<f64>,    // binary_score
            Option<f64>,    // raytrace_score
            Option<f64>,    // lab_l
            Option<f64>,    // lab_a
            Option<f64>,    // lab_b
            Option<f64>,    // aspect_ratio
            Option<String>, // palette_json
        )> = sqlx::query_as(
            "SELECT phash, sketch_score, binary_score, raytrace_score, lab_l, lab_a, lab_b, aspect_ratio, palette_json 
             FROM images 
             WHERE path = ?1 AND modified = ?2 AND lab_l IS NOT NULL"
        )
        .bind(&path_str)
        .bind(modified_ts)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((phash, sketch, binary, raytrace, l, a, b, ar, palette_json)) = row {
            let phash_str = phash.unwrap_or_default();
            let dominant_colors: Vec<[f32; 3]> = palette_json
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            // Reconstruct phash_bits
            let phash_bits = if !phash_str.is_empty() {
                crate::engine::auto_group::phash_to_bits_pub(&phash_str)
            } else {
                None
            };

            let feat = crate::engine::auto_group::ImageFeature {
                path: path.to_path_buf(),
                time: modified_ts as f32,
                l: l.unwrap_or(0.0) as f32,
                a: a.unwrap_or(0.0) as f32,
                b: b.unwrap_or(0.0) as f32,
                aspect_ratio: ar.unwrap_or(1.0) as f32,
                phash_bits,
                dominant_colors,
                sketch_score: sketch.unwrap_or(0.0) as f32,
                binary_score: binary.unwrap_or(0.0) as f32,
                raytrace_score: raytrace.unwrap_or(0.0) as f32,
            };

            return Ok(Some((feat, phash_str)));
        }

        Ok(None)
    }

    pub async fn insert_full_feature(
        &self, 
        file: &FileInfo, 
        feat: &crate::engine::auto_group::ImageFeature,
        phash: &str,
    ) -> Result<()> {
        let modified_ts = file.modified
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let palette_json = serde_json::to_string(&feat.dominant_colors).unwrap_or_default();
            
        sqlx::query(
            "INSERT INTO images (path, size, modified, exif_json, phash, sketch_score, binary_score, raytrace_score, lab_l, lab_a, lab_b, aspect_ratio, palette_json) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(path) DO UPDATE SET 
                size=excluded.size, modified=excluded.modified, exif_json=excluded.exif_json, 
                phash=excluded.phash, sketch_score=excluded.sketch_score, 
                binary_score=excluded.binary_score, raytrace_score=excluded.raytrace_score,
                lab_l=excluded.lab_l, lab_a=excluded.lab_a, lab_b=excluded.lab_b,
                aspect_ratio=excluded.aspect_ratio, palette_json=excluded.palette_json"
        )
        .bind(file.path.to_string_lossy().to_string())
        .bind(file.size as i64)
        .bind(modified_ts)
        .bind("{}")
        .bind(if phash.is_empty() { None } else { Some(phash) })
        .bind(feat.sketch_score as f64)
        .bind(feat.binary_score as f64)
        .bind(feat.raytrace_score as f64)
        .bind(feat.l as f64)
        .bind(feat.a as f64)
        .bind(feat.b as f64)
        .bind(feat.aspect_ratio as f64)
        .bind(palette_json)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }

    pub async fn insert_image_metadata(
        &self, 
        file: FileInfo, 
        phash: Option<String>,
        sketch: Option<f64>,
        binary: Option<f64>,
        raytrace: Option<f64>
    ) -> Result<()> {
        let modified_ts = file.modified
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
            
        sqlx::query(
            "INSERT INTO images (path, size, modified, exif_json, phash, sketch_score, binary_score, raytrace_score) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(path) DO UPDATE SET 
                size=excluded.size, modified=excluded.modified, exif_json=excluded.exif_json, 
                phash=excluded.phash, sketch_score=excluded.sketch_score, 
                binary_score=excluded.binary_score, raytrace_score=excluded.raytrace_score"
        )
        .bind(file.path.to_string_lossy().to_string())
        .bind(file.size as i64)
        .bind(modified_ts)
        .bind("{}")
        .bind(phash)
        .bind(sketch)
        .bind(binary)
        .bind(raytrace)
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
