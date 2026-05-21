use std::path::{Path, PathBuf};
use std::io;
use tokio::fs::{self, File};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use serde_json::Value;
use sha2::{Sha256, Digest};
use anyhow::{Result, anyhow};

pub struct SessionService {
    projects_dir: PathBuf,
}

impl SessionService {
    pub fn new(projects_dir: PathBuf) -> Self {
        Self { projects_dir }
    }

    /// Validate that path component doesn't contain traversal sequences.
    fn validate_path_component(&self, component: &str) -> Result<String> {
        if component.contains("..") || component.contains('/') || component.contains('\\') {
            return Err(anyhow!("Invalid path component: traversal detected"));
        }
        if component.starts_with('.') {
            return Err(anyhow!("Hidden paths not allowed"));
        }
        if component.trim().is_empty() || component == "." || component == ".." {
            return Err(anyhow!("Invalid path component: empty or restricted name"));
        }
        Ok(component.to_string())
    }

    /// Safely resolve a filepath within the projects directory.
    pub async fn resolve_session_path(&self, project_folder: &str, session_id: &str) -> Result<PathBuf> {
        let project_folder = self.validate_path_component(project_folder)?;
        let session_id = self.validate_path_component(session_id)?;

        let mut path = self.projects_dir.clone();
        path.push(project_folder);
        path.push(format!("{}.jsonl", session_id));

        // Canonicalize to check if it's still within projects_dir
        let canonical_projects = fs::canonicalize(&self.projects_dir).await?;
        let canonical_file = fs::canonicalize(&path).await.map_err(|e| anyhow!("File not found or inaccessible: {}", e))?;

        if !canonical_file.starts_with(&canonical_projects) {
            return Err(anyhow!("Path traversal detected outside projects directory"));
        }

        Ok(canonical_file)
    }

    /// Parse JSONL file with safety limits to prevent DoS.
    pub async fn parse_jsonl_file(&self, filepath: &Path) -> Result<Vec<Value>> {
        const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
        const MAX_ENTRIES: usize = 10000;

        let metadata = fs::metadata(filepath).await?;
        if metadata.len() > MAX_FILE_SIZE {
            return Err(anyhow!("File too large: {} bytes (max {} bytes)", metadata.len(), MAX_FILE_SIZE));
        }

        let file = File::open(filepath).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut entries = Vec::new();

        while let Some(line) = lines.next_line().await? {
            if entries.len() >= MAX_ENTRIES {
                return Err(anyhow!("Too many entries in session file (max {})", MAX_ENTRIES));
            }
            if !line.trim().is_empty() {
                if let Ok(json) = serde_json::from_str(&line) {
                    entries.push(json);
                }
            }
        }

        Ok(entries)
    }

    /// Calculate SHA-256 hash of file content (optimized for large files).
    pub async fn get_file_hash(&self, filepath: &Path) -> Result<String> {
        let metadata = fs::metadata(filepath).await?;
        let size = metadata.len();
        let mtime = metadata.modified()?.duration_since(std::time::UNIX_EPOCH)?.as_secs();

        let mut hasher = Sha256::new();

        if size < 1024 * 1024 { // < 1MB, hash everything
            let content = fs::read(filepath).await?;
            hasher.update(&content);
        } else {
            // For large files, hash first 64KB and last 64KB + metadata
            let mut file = File::open(filepath).await?;
            let mut buffer = vec![0u8; 65536];

            // Read first 64KB
            let n = file.read(&mut buffer).await?;
            hasher.update(&buffer[..n]);

            // Seek to last 64KB
            if size > 65536 {
                let pos = size - 65536;
                tokio::io::AsyncSeekExt::seek(&mut file, io::SeekFrom::Start(pos)).await?;
                let n = file.read(&mut buffer).await?;
                hasher.update(&buffer[..n]);
            }
        }

        // Include metadata in hash
        hasher.update(format!("{}:{}", size, mtime).as_bytes());

        Ok(hex::encode(hasher.finalize()))
    }
}
