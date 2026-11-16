//! Cache de mídia, transações serializadas e descarte por último uso.
use crate::protocol::{Result, error};
use std::{
    fs::{self, File, Metadata},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

pub const LIMIT: u64 = 5_000_000_000;

#[derive(Clone)]
pub struct Cache {
    directory: PathBuf,
    gate: Arc<Mutex<()>>,
    limit: u64,
}

fn failed() -> crate::protocol::Error {
    error("cache_io", "Não foi possível acessar ou reduzir o cache de mídia.")
}
fn linked(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}
impl Cache {
    pub fn new(directory: PathBuf) -> Result<Self> {
        let metadata = fs::symlink_metadata(&directory).map_err(|_| failed())?;
        if !metadata.is_dir() || linked(&metadata) {
            return Err(failed());
        }
        let cache = Self { directory: fs::canonicalize(directory).map_err(|_| failed())?, gate: Arc::new(Mutex::new(())), limit: LIMIT };
        cache.prune()?;
        Ok(cache)
    }
    fn validate_root(&self) -> Result<()> {
        let metadata = fs::symlink_metadata(&self.directory).map_err(|_| failed())?;
        if !metadata.is_dir() || linked(&metadata) || fs::canonicalize(&self.directory).map_err(|_| failed())? != self.directory {
            return Err(failed());
        }
        Ok(())
    }
    /// O arquivo usado e os temporários da operação ficam protegidos da limpeza.
    pub fn entry<T>(&self, name: &str, operation: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
        let mut components = Path::new(name).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err(failed());
        }
        let _guard = self.gate.lock().map_err(|_| failed())?;
        self.validate_root()?;
        let path = self.directory.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.is_file() || linked(&metadata) => return Err(failed()),
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(failed()),
            _ => {}
        }
        let result = operation(&path);
        self.validate_root()?;
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if !metadata.is_file() || linked(&metadata) {
                return Err(failed());
            }
            File::options().write(true).open(&path).and_then(|f| f.set_modified(SystemTime::now())).map_err(|_| failed())?;
        }
        self.trim()?;
        result
    }
    pub fn prune(&self) -> Result<()> {
        let _guard = self.gate.lock().map_err(|_| failed())?;
        self.validate_root()?;
        self.trim()
    }
    fn trim(&self) -> Result<()> {
        let mut files = Vec::new();
        let mut total = 0u64;
        let mut directories = vec![self.directory.clone()];
        while let Some(directory) = directories.pop() {
            self.validate_directory(&directory)?;
            for entry in fs::read_dir(directory).map_err(|_| failed())? {
                let entry = entry.map_err(|_| failed())?;
                let metadata = fs::symlink_metadata(entry.path()).map_err(|_| failed())?;
                if linked(&metadata) {
                    continue;
                }
                if metadata.is_dir() {
                    directories.push(entry.path());
                } else if metadata.is_file() {
                    total = total.saturating_add(metadata.len());
                    files.push((metadata.modified().unwrap_or(UNIX_EPOCH), entry.path(), metadata.len()));
                }
            }
        }
        if total <= self.limit {
            return Ok(());
        }
        files.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        for (_, path, size) in files {
            if total <= self.limit {
                break;
            }
            self.validate_root()?;
            self.validate_directory(path.parent().ok_or_else(failed)?)?;
            let metadata = fs::symlink_metadata(&path).map_err(|_| failed())?;
            if metadata.is_file() && !linked(&metadata) {
                fs::remove_file(&path).map_err(|_| failed())?;
                total = total.saturating_sub(size);
            }
        }
        Ok(())
    }
    fn validate_directory(&self, directory: &Path) -> Result<()> {
        let metadata = fs::symlink_metadata(directory).map_err(|_| failed())?;
        let canonical = fs::canonicalize(directory).map_err(|_| failed())?;
        if !metadata.is_dir() || linked(&metadata) || canonical != directory || !canonical.starts_with(&self.directory) {
            return Err(failed());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn file(path: &Path, bytes: &[u8], age: u64) {
        fs::write(path, bytes).unwrap();
        File::options().write(true).open(path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(age)).unwrap();
    }
    #[test]
    fn last_use_controls_eviction_and_paths_stay_inside_cache() {
        let directory = tempfile::tempdir().unwrap();
        let cache_dir = directory.path().join("cache");
        fs::create_dir(&cache_dir).unwrap();
        let mut cache = Cache::new(cache_dir.clone()).unwrap();
        cache.limit = 100;
        let outside = directory.path().join("preservar.bin");
        file(&outside, b"fora do cache", 100);
        let stamp = fs::metadata(&outside).unwrap().modified().unwrap();
        file(&cache_dir.join("old.bin"), &[1; 40], 30);
        file(&cache_dir.join("middle.bin"), &[2; 40], 20);
        file(&cache_dir.join("new.bin"), &[3; 40], 10);
        cache
            .entry("old.bin", |p| {
                assert_eq!(fs::read(p).unwrap(), [1; 40]);
                Ok(())
            })
            .unwrap();
        assert!(cache_dir.join("old.bin").is_file());
        assert!(!cache_dir.join("middle.bin").exists());
        assert!(cache_dir.join("new.bin").is_file());
        assert!(cache.entry("../preservar.bin", |_| Ok(())).is_err());
        assert!(cache.entry(outside.to_str().unwrap(), |_| Ok(())).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"fora do cache");
        assert_eq!(fs::metadata(&outside).unwrap().modified().unwrap(), stamp);
        let inside = cache_dir.join("subdirectory");
        fs::create_dir(&inside).unwrap();
        file(&inside.join("old-nested.bin"), &[9; 200], 100);
        cache.prune().unwrap();
        assert!(!inside.join("old-nested.bin").exists());
        assert!(inside.is_dir());
    }
    #[test]
    fn entries_and_cleanup_serialize_concurrent_writes() {
        let directory = tempfile::tempdir().unwrap();
        let mut cache = Cache::new(directory.path().into()).unwrap();
        cache.limit = 100;
        thread_scope(&cache);
        let total: u64 = fs::read_dir(directory.path()).unwrap().map(|entry| entry.unwrap().metadata().unwrap().len()).sum();
        assert!(total <= 100);
    }
    fn thread_scope(cache: &Cache) {
        std::thread::scope(|scope| {
            for index in 0..8 {
                scope.spawn(move || {
                    cache
                        .entry(&format!("item-{index}.bin"), |path| {
                            fs::write(path, [index; 40]).unwrap();
                            std::thread::sleep(Duration::from_millis(2));
                            assert_eq!(fs::read(path).unwrap(), [index; 40]);
                            Ok(())
                        })
                        .unwrap();
                });
            }
        });
    }
    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) {
        let result = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(link).arg(target).output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    }
    #[test]
    #[cfg(windows)]
    fn junctions_are_never_followed_or_used_as_cache_roots() {
        let directory = tempfile::tempdir().unwrap();
        let outside = directory.path().join("outside");
        let root = directory.path().join("cache");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep.bin"), b"preservar").unwrap();
        junction(&root.join("link"), &outside);
        let mut cache = Cache::new(root.clone()).unwrap();
        cache.limit = 0;
        cache.prune().unwrap();
        assert!(cache.entry("link", |_| Ok(())).is_err());
        assert!(Cache::new(root.join("link")).is_err());
        assert_eq!(fs::read(outside.join("keep.bin")).unwrap(), b"preservar");
    }
    #[test]
    #[cfg(windows)]
    #[ignore = "Aceite M2.3: arquivos sparse com 6 GB lógicos em MOTOR_M23_OUTPUT, limiar real de 5 GB."]
    fn cache_limit_five_gigabytes_acceptance() {
        let output = PathBuf::from(std::env::var("MOTOR_M23_OUTPUT").expect("MOTOR_M23_OUTPUT"));
        let root = output.join("cache-limit");
        fs::create_dir_all(&root).unwrap();
        let outside = output.join("outside-sentinel.bin");
        file(&outside, b"arquivo fora do cache", 100);
        let stamp = fs::metadata(&outside).unwrap().modified().unwrap();
        for (name, age) in [("peaks-old.bin", 30), ("thumbs-new.json", 10)] {
            let path = root.join(name);
            let file = File::create(&path).unwrap();
            let sparse = std::process::Command::new("fsutil").args(["sparse", "setflag"]).arg(&path).output().unwrap();
            assert!(sparse.status.success(), "{}", String::from_utf8_lossy(&sparse.stderr));
            file.set_len(3_000_000_000).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(age)).unwrap();
        }
        let started = Instant::now();
        Cache::new(root.clone()).unwrap();
        let milliseconds = started.elapsed().as_secs_f64() * 1000.0;
        let after: u64 = fs::read_dir(&root).unwrap().map(|e| e.unwrap().metadata().unwrap().len()).sum();
        assert!(!root.join("peaks-old.bin").exists());
        assert!(root.join("thumbs-new.json").exists());
        assert_eq!(after, 3_000_000_000);
        assert_eq!(fs::read(&outside).unwrap(), b"arquivo fora do cache");
        assert_eq!(fs::metadata(&outside).unwrap().modified().unwrap(), stamp);
        let report = serde_json::json!({"beforeBytes":6_000_000_000u64,"afterBytes":after,"limitBytes":LIMIT,"elapsedMs":milliseconds,"outsideUntouched":true,"sparse":true});
        fs::write(output.join("cache-measurements.json"), report.to_string()).unwrap();
        eprintln!("M2.3 cache {report}");
    }
}
