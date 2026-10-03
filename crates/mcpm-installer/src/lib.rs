use std::path::{Path, PathBuf};

use futures::StreamExt;
use mcpm_core::{LockedMod, Lockfile, McpmError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use tokio::io::AsyncWriteExt;

pub struct Installer {
    cache_dir: PathBuf,
    mods_dir: PathBuf,
    state_dir: PathBuf,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallState {
    managed_files: Vec<String>,
}

pub struct InstallResult {
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
    pub manual: Vec<(String, Option<String>)>,
    pub errors: Vec<(String, String)>,
}

impl Installer {
    pub fn new(project_dir: &Path) -> Result<Self, McpmError> {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from(".mcpm_cache"))
            .join("mcpm");
        let mods_dir = project_dir.join("mods");
        let state_dir = project_dir.join(".mcpm");
        let client = reqwest::Client::builder()
            .user_agent("mcpm/0.1.0")
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .build()
            .expect("failed to build HTTP client");
        Ok(Self {
            cache_dir,
            mods_dir,
            state_dir,
            client,
        })
    }

    pub fn mods_dir(&self) -> &Path {
        &self.mods_dir
    }

    pub async fn install(&self, lockfile: &Lockfile) -> Result<InstallResult, McpmError> {
        tokio::try_join!(
            async { tokio::fs::create_dir_all(&self.cache_dir).await },
            async { tokio::fs::create_dir_all(&self.mods_dir).await },
            async { tokio::fs::create_dir_all(&self.state_dir).await },
        )
        .map_err(|e| McpmError::Install(format!("failed to create directories: {}", e)))?;

        let mut installed = Vec::new();
        let mut skipped = Vec::new();
        let mut manual = Vec::new();
        let mut errors = Vec::new();

        let total = lockfile.mods.len();
        let pb = indicatif::ProgressBar::new(total as u64);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("{pos}/{len} [{bar:40.cyan/blue}] {msg}")
                .expect("invalid progress style")
                .progress_chars("#>-"),
        );

        for m in &lockfile.mods {
            pb.set_message(m.filename.clone());

            if m.manual_download || m.url.is_empty() {
                manual.push((m.filename.clone(), m.website_url.clone()));
                pb.inc(1);
                continue;
            }

            match self.install_mod(m).await {
                Ok(was_downloaded) => {
                    if was_downloaded {
                        installed.push(m.filename.clone());
                    } else {
                        skipped.push(m.filename.clone());
                    }
                }
                Err(e) => {
                    errors.push((m.filename.clone(), e.to_string()));
                }
            }
            pb.inc(1);
        }
        pb.finish_and_clear();

        let managed_files: Vec<String> = lockfile
            .mods
            .iter()
            .map(|m| format!("mods/{}", m.filename))
            .collect();
        self.save_state(&managed_files).await?;

        Ok(InstallResult {
            installed,
            skipped,
            manual,
            errors,
        })
    }

    async fn install_mod(&self, m: &LockedMod) -> Result<bool, McpmError> {
        let cache_path = self.cache_path(m);
        let mods_path = self.mods_dir.join(&m.filename);

        if tokio::fs::metadata(&cache_path).await.is_err() {
            self.download(m).await?;
        }

        let hash_ok = self.verify_hash_ok(m, &cache_path).await;
        if !hash_ok {
            tracing::warn!(
                "cached file for {} has wrong hash, re-downloading",
                m.filename
            );
            let _ = tokio::fs::remove_file(&cache_path).await;
            self.download(m).await?;
            self.verify_hash(m, &cache_path).await?;
        }

        let mods_exists = tokio::fs::metadata(&mods_path).await.is_ok();
        if mods_exists {
            if let Some(expected) = &m.sha512 {
                let existing_hash = self.compute_file_hash(&mods_path).await?;
                if existing_hash.eq_ignore_ascii_case(expected) {
                    return Ok(false);
                }
            } else if let Some(expected) = &m.sha1 {
                let existing_hash = self.compute_file_hash_sha1(&mods_path).await?;
                if existing_hash.eq_ignore_ascii_case(expected) {
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        }

        tokio::fs::copy(&cache_path, &mods_path)
            .await
            .map_err(|e| {
                McpmError::Install(format!("failed to copy {} to mods/: {}", m.filename, e))
            })?;

        Ok(true)
    }

    async fn download(&self, m: &LockedMod) -> Result<(), McpmError> {
        tracing::info!("downloading {}", m.filename);
        let response =
            self.client.get(&m.url).send().await.map_err(|e| {
                McpmError::Install(format!("failed to download {}: {}", m.filename, e))
            })?;

        if !response.status().is_success() {
            return Err(McpmError::Install(format!(
                "failed to download {}: HTTP {}",
                m.filename,
                response.status()
            )));
        }

        tracing::debug!(
            "downloading {}: content-length={:?}, content-encoding={:?}",
            m.filename,
            response.headers().get("content-length"),
            response.headers().get("content-encoding"),
        );

        let cache_path = self.cache_path(m);
        let tmp_path = cache_path.with_extension(".tmp");

        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(|e| McpmError::Install(format!("failed to create temp file: {}", e)))?;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                McpmError::Install(format!("failed to read download stream: {}", e))
            })?;
            file.write_all(&chunk)
                .await
                .map_err(|e| McpmError::Install(format!("failed to write temp file: {}", e)))?;
        }

        drop(file);

        tokio::fs::rename(&tmp_path, &cache_path)
            .await
            .inspect_err(|_| {
                let tmp = tmp_path.clone();
                tokio::spawn(async move {
                    let _ = tokio::fs::remove_file(&tmp).await;
                });
            })
            .map_err(|e| McpmError::Install(format!("failed to finalize cache file: {}", e)))?;

        Ok(())
    }

    async fn verify_hash_ok(&self, m: &LockedMod, path: &Path) -> bool {
        if let Some(expected) = &m.sha512 {
            self.compute_file_hash(path)
                .await
                .map(|actual| actual.eq_ignore_ascii_case(expected))
                .unwrap_or(false)
        } else if let Some(expected) = &m.sha1 {
            self.compute_file_hash_sha1(path)
                .await
                .map(|actual| actual.eq_ignore_ascii_case(expected))
                .unwrap_or(false)
        } else {
            true
        }
    }

    async fn verify_hash(&self, m: &LockedMod, path: &Path) -> Result<(), McpmError> {
        if let Some(expected) = &m.sha512 {
            let actual = self.compute_file_hash(path).await?;
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(McpmError::HashMismatch {
                    filename: m.filename.clone(),
                    expected: expected.clone(),
                    actual,
                });
            }
        } else if let Some(expected) = &m.sha1 {
            let actual = self.compute_file_hash_sha1(path).await?;
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(McpmError::HashMismatch {
                    filename: m.filename.clone(),
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        Ok(())
    }

    async fn compute_file_hash(&self, path: &Path) -> Result<String, McpmError> {
        let data = tokio::fs::read(path).await.map_err(|e| {
            McpmError::Install(format!("failed to read file {}: {}", path.display(), e))
        })?;
        let mut hasher = Sha512::new();
        hasher.update(&data);
        Ok(hex::encode(hasher.finalize()))
    }

    async fn compute_file_hash_sha1(&self, path: &Path) -> Result<String, McpmError> {
        let data = tokio::fs::read(path).await.map_err(|e| {
            McpmError::Install(format!("failed to read file {}: {}", path.display(), e))
        })?;
        let mut hasher = sha1::Sha1::new();
        hasher.update(&data);
        Ok(hex::encode(hasher.finalize()))
    }

    fn cache_path(&self, m: &LockedMod) -> PathBuf {
        let key = m
            .sha512
            .as_deref()
            .or(m.sha1.as_deref())
            .unwrap_or(&m.filename);
        self.cache_dir.join(key)
    }

    async fn save_state(&self, managed_files: &[String]) -> Result<(), McpmError> {
        let state = InstallState {
            managed_files: managed_files.to_vec(),
        };
        let state_path = self.state_dir.join("state.json");
        let json = serde_json::to_string_pretty(&state)
            .map_err(|e| McpmError::Install(format!("failed to serialize state: {}", e)))?;
        tokio::fs::write(&state_path, json)
            .await
            .map_err(|e| McpmError::Install(format!("failed to write state: {}", e)))?;
        Ok(())
    }

    pub async fn load_state(&self) -> Result<Option<InstallState>, McpmError> {
        let state_path = self.state_dir.join("state.json");
        if !state_path.exists() {
            return Ok(None);
        }
        let data = tokio::fs::read_to_string(&state_path)
            .await
            .map_err(|e| McpmError::Install(format!("failed to read state: {}", e)))?;
        let state: InstallState = serde_json::from_str(&data)
            .map_err(|e| McpmError::Install(format!("failed to parse state: {}", e)))?;
        Ok(Some(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpm_core::{
        LockEnvironment, LockReason, LockedMod, Lockfile, ReleaseType, Side, SourceKind,
    };

    fn manual_mod(filename: &str) -> LockedMod {
        LockedMod {
            name: "Manual Mod".to_string(),
            source: SourceKind::CurseForge,
            project_id: "42".to_string(),
            slug: None,
            version_id: "100".to_string(),
            version_number: "1.0.0".to_string(),
            version_type: Some(ReleaseType::Release),
            filename: filename.to_string(),
            url: String::new(),
            sha512: None,
            sha1: None,
            size: None,
            side: Side::Both,
            reason: LockReason::Requested,
            dependencies: vec![],
            manual_download: true,
            website_url: Some("https://www.curseforge.com/minecraft/mc-mods/manual".to_string()),
        }
    }

    #[tokio::test]
    async fn test_manual_download_mod_is_skipped_not_failed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let installer = Installer::new(dir.path()).expect("installer");

        let lockfile = Lockfile::new(
            "0.1.0",
            LockEnvironment {
                minecraft: "1.20.1".to_string(),
                loader: mcpm_core::LoaderKind::Forge,
                loader_version: None,
                side: Side::Both,
            },
        );
        let mut lockfile = lockfile;
        lockfile.mods.push(manual_mod("manual-mod-1.0.0.jar"));

        let result = installer.install(&lockfile).await.expect("install");

        assert!(result.installed.is_empty());
        assert!(result.errors.is_empty());
        assert_eq!(result.manual.len(), 1);
        assert_eq!(result.manual[0].0, "manual-mod-1.0.0.jar");
        assert!(result.manual[0].1.is_some());
        assert!(!installer.mods_dir().join("manual-mod-1.0.0.jar").exists());
    }
}
