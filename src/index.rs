use anyhow::anyhow;
use bytes::Bytes;
use compio::runtime::spawn_blocking;
use minijinja::{Environment, context};
use moka::future::Cache;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Clone, Debug)]
pub struct Index {
    last_mtime: SystemTime,
    rendered_html: Bytes,
}

#[derive(Debug)]
pub struct IndexCache {
    jinja_env: &'static Environment<'static>,
    inner: Cache<Arc<PathBuf>, Arc<Index>>,
}

impl IndexCache {
    pub fn new(jinja_env: &'static Environment<'static>, max_capacity: u64) -> Self {
        Self {
            jinja_env,
            inner: Cache::builder().max_capacity(max_capacity).build(),
        }
    }
    pub async fn insert(&self, path: Arc<PathBuf>, index: Arc<Index>) {
        self.inner.insert(path, index).await;
    }

    pub async fn render_index(&self, dir_path: &PathBuf) -> anyhow::Result<Bytes> {
        let metadata = fs::metadata(dir_path)?;

        let current_mtime = metadata.modified()?;

        if let Some(cached_index) = self
            .inner
            .get(dir_path)
            .await
            .filter(|idx| idx.last_mtime == current_mtime)
        {
            return Ok(cached_index.rendered_html.clone());
        };

        let owned_dir_path = dir_path.clone();
        let files = spawn_blocking(move || -> anyhow::Result<Vec<String>> {
            let entries = fs::read_dir(owned_dir_path)?;
            let mut files = Vec::new();

            for entry in entries {
                let entry = entry?;
                if let Ok(name) = entry.file_name().into_string()
                    && !name.starts_with(".")
                {
                    files.push(name);
                }
            }
            files.sort_unstable();
            Ok(files)
        })
        .await
        .map_err(|e| anyhow!("JoinError: {:?}", e))??;

        let template = self
            .jinja_env
            .get_template("index")
            .map_err(|e| anyhow!("Failed to get template: {}", e))?;
        let rendered = template
            .render(context!(files, dir_path => dir_path.display().to_string()))
            .map_err(|e| anyhow!("Failed to render template: {}", e))?;

        let rendered = Bytes::from(rendered);
        self.insert(
            Arc::new(dir_path.clone()),
            Arc::new(Index {
                last_mtime: current_mtime,
                rendered_html: rendered.clone(),
            }),
        )
        .await;
        Ok(rendered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::fs::File;
    use tempfile::tempdir;

    static JINJA_ENV: Lazy<Environment<'static>> = Lazy::new(|| {
        let mut env = Environment::new();
        env.add_template("index", include_str!("../template/index.jinja"))
            .unwrap();
        env
    });

    #[compio::test]
    async fn test_render_index() {
        // Setup: Create a temporary directory and some files
        let dir = tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        File::create(dir.path().join("file1.txt")).unwrap();
        File::create(dir.path().join("file2.txt")).unwrap();
        File::create(dir.path().join(".hidden")).unwrap();

        // --- First render (cache miss) ---
        let index_cache = IndexCache::new(&JINJA_ENV, 100);
        let result1 = index_cache.render_index(&dir_path).await.unwrap();
        let html1 = String::from_utf8(result1.to_vec()).unwrap();

        // Assertions
        assert!(html1.contains("file1.txt"));
        assert!(html1.contains("file2.txt"));
        assert!(!html1.contains(".hidden"));

        // --- Second render (cache hit) ---
        // To ensure we get a cache hit, we render again without changing the directory.
        let result2 = index_cache.render_index(&dir_path).await.unwrap();
        assert_eq!(result1, result2);

        // --- Third render (cache miss after modification) ---
        // Modify the directory by adding a new file
        File::create(dir.path().join("file3.txt")).unwrap();
        // We need to sleep a bit to ensure the modification time is different.
        std::thread::sleep(std::time::Duration::from_millis(10));

        let result3 = index_cache.render_index(&dir_path).await.unwrap();
        let html3 = String::from_utf8(result3.to_vec()).unwrap();

        // Assertions for the updated render
        assert!(html3.contains("file1.txt"));
        assert!(html3.contains("file2.txt"));
        assert!(html3.contains("file3.txt"));
        assert!(!html3.contains(".hidden"));
        assert_ne!(html1, html3);
    }

    #[compio::test]
    async fn test_render_index_not_found() {
        let dir_path = PathBuf::from("non_existent_directory_for_testing");
        let index_cache = IndexCache::new(&JINJA_ENV, 100);
        let result = index_cache.render_index(&dir_path).await;
        assert!(result.is_err());
    }
}
