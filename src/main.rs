extern crate yaml_rust;

#[cfg(unix)]
extern crate daemonize_me;
#[cfg(unix)]
use daemonize_me::Daemon;
use home::home_dir;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher, Event};
use std::{path::{Path, PathBuf}, fs::{self}, collections::{HashMap, HashSet}, thread, time::Duration};
#[cfg(unix)]
use std::{fs::File, process::exit};

use yaml_rust::{Yaml, YamlLoader};


#[derive(Clone)]
struct WatchDir {
    path: PathBuf,
    file_types: HashMap<String, String>,
}

#[derive(Clone, Copy)]
struct StabilityConfig {
    interval: Duration,
    required_stable_ticks: u32,
}

fn main() {
    start_daemon();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let config_path = home_dir().unwrap().join(".config/watch-dir/config.yaml");
    if !config_path.exists() {
        load_config();
    }
    // reading the config file from ~/.config/watch-dir/config.yaml
    let s = fs::read_to_string(config_path).unwrap();

    let doc = YamlLoader::load_from_str(&s).unwrap();
    let config = &doc[0]["config"];

    let default_file_types = parse_file_types(&config["file-types"]);

    let binding = config["watch"].as_vec();
    let watch_entries = match &binding {
        Some(x) => x,
        None => panic!("No files to watch")
    };

    let mut dirs: Vec<WatchDir> = Vec::new();
    for entry in watch_entries.iter() {
        let raw_path = match entry["path"].as_str() {
            Some(x) => x,
            None => panic!("Watch entry missing 'path'")
        };

        let path = resolve_path(raw_path);

        let file_types = if entry["file-types"].is_badvalue() {
            default_file_types.clone()
        } else {
            parse_file_types(&entry["file-types"])
        };

        dirs.push(WatchDir { path, file_types });
    }

    let ignore_extensions = parse_ignore_extensions(&config["ignore-extensions"]);
    let stability = parse_stability(&config["stability"]);

    log::info!("Watching {:?}", dirs.iter().map(|d| &d.path).collect::<Vec<_>>());
    if let Err(error) = watch(dirs, ignore_extensions, stability) {
        log::error!("Error: {error:?}");
    }
}

fn resolve_path(raw_path: &str) -> PathBuf {
    if let Some(stripped) = raw_path.strip_prefix("~/") {
        home::home_dir().unwrap().join(stripped)
    } else {
        PathBuf::from(raw_path)
    }
}

fn parse_file_types(node: &Yaml) -> HashMap<String, String> {
    let mut file_type_hash: HashMap<String, String> = HashMap::new();
    let hash = match node.as_hash() {
        Some(x) => x,
        None => panic!("No file types")
    };
    for (key, value) in hash.iter() {
        let key = match key.as_str() {
            Some(x) => x,
            None => panic!("No key")
        };
        let value = match value.as_vec() {
            Some(x) => x,
            None => panic!("No value")
        };
        for file_type in value.iter() {
            let file_type = match file_type.as_str() {
                Some(x) => x,
                None => panic!("No file type")
            };
            file_type_hash.insert(file_type.to_lowercase(), key.to_string());
        }
    }
    file_type_hash
}

fn parse_ignore_extensions(node: &Yaml) -> HashSet<String> {
    match node.as_vec() {
        Some(x) => x.iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_lowercase())
            .collect(),
        None => HashSet::new(),
    }
}

fn parse_stability(node: &Yaml) -> StabilityConfig {
    let interval_ms = node["interval-ms"].as_i64().unwrap_or(1000) as u64;
    let required_stable_ticks = node["required-stable-ticks"].as_i64().unwrap_or(2) as u32;
    StabilityConfig {
        interval: Duration::from_millis(interval_ms),
        required_stable_ticks,
    }
}


fn watch(dirs: Vec<WatchDir>, ignore_extensions: HashSet<String>, stability: StabilityConfig) -> notify::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();

    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.

    for dir in dirs.iter() {
        watcher.watch(dir.path.as_path(), RecursiveMode::Recursive)?;
    }

    for res in rx {
        match res {
            Ok(event) =>{
                match event.kind {
                    notify::event::EventKind::Create(file)=>{
                        // check if the created file is a not a directory
                        match file {
                            notify::event::CreateKind::File=>{
                                handle_new_file(event, &dirs, &ignore_extensions, stability);
                            },
                            _=>{}
                        }
                    },
                    _ => {}
                }
            },
            Err(error) => log::error!("Error: {error:?}"),
        }
    }

    Ok(())
}

fn handle_new_file(event: Event, dirs: &[WatchDir], ignore_extensions: &HashSet<String>, stability: StabilityConfig) {
    let path = event.paths[0].clone();

    let parent_dir = match path.parent() {
        Some(x) => x,
        None => return,
    };
    let matched_dir = dirs.iter().find(|d| d.path == parent_dir);
    let matched_dir = match matched_dir {
        Some(x) => x.clone(),
        None => return,
    };

    let extension = match path.extension().and_then(|e| e.to_str()) {
        Some(x) => x.to_lowercase(),
        None => return, // extensionless file / dotfile, nothing to sort by
    };

    if ignore_extensions.contains(&extension) {
        return;
    }

    thread::spawn(move || {
        if !wait_until_stable(&path, stability.interval, stability.required_stable_ticks) {
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

fn new_file_created(path: &Path, extension: &str, file_types: &HashMap<String, String>) -> notify::Result<()> {
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
        fs::create_dir_all(move_to_dir.clone()).unwrap();
    }
    let file_name = match path.file_name() {
        Some(x) => x,
        None => return Ok(()),
    };
    let move_to_dir = move_to_dir.join(file_name);
    fs::rename(path, move_to_dir).unwrap();
    Ok(())
}

#[cfg(unix)]
fn start_daemon() {
    // check if the daemon is already running
    let pid_file = "watch-dir.pid";
    let pid_file = Path::new(pid_file);
    if pid_file.exists() {
        eprintln!("The daemon is already running");
        exit(-1);
    }
    let stdout = File::create("/tmp/info.log").unwrap();
    let stderr = File::create("/tmp/err.log").unwrap();
    let daemon = Daemon::new()
        .pid_file("watch-dir.pid", Some(false))
        .umask(0o000)
        .work_dir(".")
        .stdout(stdout)
        .stderr(stderr)
        // Start the daemon and calls the hooks
        .start();

    match daemon {
        Ok(_) => println!("Daemonized with success"),
        Err(e) => {
            eprintln!("Error, {}", e);
            exit(-1);
        },
    }
}

#[cfg(not(unix))]
fn start_daemon() {
    log::info!("Daemonizing not supported on this platform — running in foreground. Use Task Scheduler (Windows) or launchd (macOS) to background this.");
}


fn load_config(){
    let config = "
    config:
  watch:
    - path: ~/Downloads
  file-types:
    documents:
      - pdf
      - doc
      - docx
      - xls
      - xlsx
      - ppt
      - pptx
      - txt
    images:
      - jpg
      - jpeg
      - png
      - gif
      - tiff
      - bmp
    videos:
      - mp4
      - mov
      - avi
      - mkv
      - flv
      - m4v
      - rmvb
      - rm
      - 3gp
      - mpg
      - mpeg
      - webm
    audios:
      - mp3
      - wav
      - wma
      - ogg
      - m4a
      - aac
      - aiff
  ignore-extensions:
    - crdownload
    - part
    - download
    - opdownload
    - tmp
    - partial
  stability:
    interval-ms: 1000
    required-stable-ticks: 2
    ";
    let config_path = home_dir().unwrap().join(".config/watch-dir/config.yaml");
    if !config_path.exists() {
        fs::create_dir_all(home_dir().unwrap().join(".config/watch-dir")).unwrap();
        fs::write(config_path,config).unwrap();
    }

}
