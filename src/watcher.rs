use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::{fs, io};

use crate::config::{Config, Stability};

#[derive(Clone)]
pub struct RuntimeWatch {
    pub path: PathBuf,
    pub file_types: HashMap<String, String>,
}

pub struct RuntimeConfig {
    pub dirs: Vec<RuntimeWatch>,
    pub ignore_extensions: HashSet<String>,
    pub stability: Stability,
}

/// Handle to a watcher running on a background thread.
///
/// Dropping the handle stops the thread.
pub struct Watcher {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Watcher {
    pub fn start(config: Config) -> Watcher {
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(true));

        let rt = config.to_runtime();
        let join = {
            let stop = stop.clone();
            let paused = paused.clone();
            let running = running.clone();
            thread::spawn(move || {
                log::info!(
                    "Watching {:?}",
                    rt.dirs.iter().map(|d| &d.path).collect::<Vec<_>>()
                );
                scan_existing(&rt, &paused);
                if let Err(err) = watch(rt, stop, paused) {
                    log::error!("Watcher stopped: {err:?}");
                }
                running.store(false, Ordering::SeqCst);
            })
        };

        Watcher {
            stop,
            paused,
            running,
            join: Some(join),
        }
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Replace the running watcher with one using `config`.
    pub fn reload(&mut self, config: Config) {
        let was_paused = self.is_paused();
        self.shutdown();
        *self = Watcher::start(config);
        if was_paused {
            self.pause();
        }
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        self.running.store(false, Ordering::SeqCst);
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Run the watcher on the current thread until the process is killed.
///
/// This is the headless/daemon path: no tray, no GUI.
pub fn run_blocking(config: Config) -> io::Result<()> {
    let rt = config.to_runtime();
    log::info!(
        "Watching {:?}",
        rt.dirs.iter().map(|d| &d.path).collect::<Vec<_>>()
    );
    let paused = Arc::new(AtomicBool::new(false));
    scan_existing(&rt, &paused);
    watch(rt, Arc::new(AtomicBool::new(false)), paused)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

fn scan_existing(rt: &RuntimeConfig, paused: &Arc<AtomicBool>) {
    log::info!("Sorting existing files already present in watched directories");
    for dir in rt.dirs.iter() {
        let entries = match fs::read_dir(&dir.path) {
            Ok(entries) => entries,
            Err(err) => {
                log::warn!("Failed to read {:?}: {err:?}", dir.path);
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                handle_new_file(path, rt, paused);
            }
        }
    }
}

fn watch(
    rt: RuntimeConfig,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
) -> notify::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(tx, NotifyConfig::default())?;

    for dir in rt.dirs.iter() {
        watcher.watch(dir.path.as_path(), RecursiveMode::Recursive)?;
    }

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }

        // Poll with a timeout so the stop flag is observed promptly instead of
        // blocking forever on the next filesystem event.
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(event)) => {
                match event.kind {
                    notify::event::EventKind::Create(notify::event::CreateKind::Folder) => {}
                    notify::event::EventKind::Create(_) => {
                        // Windows' backend often reports CreateKind::Any rather than
                        // CreateKind::File, so accept anything but Folder and let
                        // handle_new_file's is_file() check filter the rest.
                        if let Some(path) = event.paths.into_iter().next() {
                            handle_new_file(path, &rt, &paused);
                        }
                    }
                    notify::event::EventKind::Modify(notify::event::ModifyKind::Name(
                        rename_mode,
                    )) => {
                        // Browsers finish a download by renaming the temp file
                        // (foo.mp4.crdownload) to its final name — that's a rename,
                        // not a Create, so it has to be handled here too.
                        use notify::event::RenameMode;
                        let target = match rename_mode {
                            RenameMode::To => event.paths.into_iter().next(),
                            RenameMode::Both => event.paths.into_iter().nth(1),
                            _ => None, // From / Any / Other — nothing new landed here
                        };
                        if let Some(path) = target {
                            handle_new_file(path, &rt, &paused);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Err(error)) => log::error!("Error: {error:?}"),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}

fn handle_new_file(path: PathBuf, rt: &RuntimeConfig, paused: &Arc<AtomicBool>) {
    if paused.load(Ordering::SeqCst) {
        return;
    }

    if path.is_dir() {
        return;
    }

    let parent_dir = match path.parent() {
        Some(x) => x,
        None => return,
    };
    let parent_dir = fs::canonicalize(parent_dir).unwrap_or_else(|_| parent_dir.to_path_buf());
    let matched_dir = rt.dirs.iter().find(|d| d.path == parent_dir);
    let matched_dir = match matched_dir {
        Some(x) => x.clone(),
        None => return,
    };

    let extension = match path.extension().and_then(|e| e.to_str()) {
        Some(x) => x.to_lowercase(),
        None => return, // extensionless file / dotfile, nothing to sort by
    };

    if rt.ignore_extensions.contains(&extension) {
        return;
    }

    let stability = rt.stability;
    let paused = paused.clone();
    thread::spawn(move || {
        if !wait_until_stable(
            &path,
            Duration::from_millis(stability.interval_ms),
            stability.required_stable_ticks,
        ) {
            return;
        }
        if paused.load(Ordering::SeqCst) {
            return;
        }
        if let Err(err) = new_file_created(&path, &extension, &matched_dir.file_types) {
            log::error!("Error: {err:?}");
        }
    });
}

fn wait_until_stable(path: &Path, interval: Duration, required_stable_ticks: u32) -> bool {
    let mut last_size: Option<u64> = None;
    let mut stable_count = 0;
    loop {
        let size = match fs::metadata(path) {
            Ok(m) => m.len(),
            Err(_) => return false, // gone (renamed/deleted mid-write)
        };
        if Some(size) == last_size {
            stable_count += 1;
            if stable_count >= required_stable_ticks {
                return true;
            }
        } else {
            stable_count = 0;
        }
        last_size = Some(size);
        thread::sleep(interval);
    }
}

fn new_file_created(
    path: &Path,
    extension: &str,
    file_types: &HashMap<String, String>,
) -> notify::Result<()> {
    log::info!("New file created: {:?}", path);

    let category = match file_types.get(extension) {
        Some(x) => x,
        None => return Ok(()),
    };

    let parent_dir = match path.parent() {
        Some(x) => x,
        None => return Ok(()),
    };

    let move_to_dir = parent_dir.join(category);
    if !move_to_dir.exists() {
        if let Err(err) = fs::create_dir_all(&move_to_dir) {
            log::error!("Failed to create {move_to_dir:?}: {err:?}");
            return Ok(());
        }
    }
    let file_name = match path.file_name() {
        Some(x) => x,
        None => return Ok(()),
    };
    let move_to_dir = move_to_dir.join(file_name);

    // On Windows, antivirus/indexer can briefly hold a lock on a just-written
    // file, making an immediate rename fail with a sharing violation — retry
    // a few times before giving up.
    let mut attempt = 0;
    loop {
        match fs::rename(path, &move_to_dir) {
            Ok(()) => break,
            Err(err) if attempt < 5 => {
                attempt += 1;
                thread::sleep(Duration::from_millis(300));
                log::warn!("Retrying move of {path:?} (attempt {attempt}): {err:?}");
            }
            Err(err) => {
                log::error!("Failed to move {path:?} to {move_to_dir:?}: {err:?}");
                break;
            }
        }
    }
    Ok(())
}
