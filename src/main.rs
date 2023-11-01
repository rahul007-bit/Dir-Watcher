extern crate daemonize_me;
extern crate yaml_rust;

use daemonize_me::Daemon;
use home::home_dir;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher, Event};
use std::{path::Path, fs::{
    self,
    File,
}, collections::HashMap, process::exit};

use yaml_rust::YamlLoader;


fn main() {
    start_daemon();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    // reading the config file from ~/.config/watch-dir/config.yaml
    let s = fs::read_to_string(home_dir().unwrap().join(".config/watch-dir/config.yaml")).unwrap();

    let doc = YamlLoader::load_from_str(&s).unwrap();
    let binding = doc[0]["config"]["watch"].as_vec();
    let files_to_watch = match &binding {
        Some(x) => x,
        None => panic!("No files to watch")
    };

    let file_types = &doc[0]["config"]["file-types"];
    let mut path: Vec<String> = Vec::new();
    for file in files_to_watch.iter() {
        let file = match file.as_str() {
            Some(x) => x,
            None => panic!("No file")
        };

        let path_to_watch = file;
        if path_to_watch.starts_with("~/") {
            let home_dir = home::home_dir().unwrap();
            let path_to_watch = path_to_watch.replace("~/", "");
            let path_to_watch = home_dir.join(path_to_watch);
            path.push(path_to_watch.to_str().unwrap().to_string());
            continue;
        }
        path.push(path_to_watch.to_string());
    }
    let file_type_hash = match file_types.as_hash() {
        Some(x) => {
            let mut file_type_hash: HashMap<String, String> = HashMap::new();
            for (key, value) in x.iter() {
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
                    file_type_hash.insert(file_type.to_string(), key.to_string());
                }
            }
            file_type_hash
        },
        None => panic!("No file types")
    };

    log::info!("Watching {path:?}");
    if let Err(error) = watch(path, file_type_hash) {
        log::error!("Error: {error:?}");
    }
}


fn watch(path: Vec<String>,file_types: HashMap<String,String>) -> notify::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();

    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.

    for path in path.iter() {
        watcher.watch(
            Path::new(path).as_ref(), RecursiveMode::Recursive)?;
    }

    for res in rx {
        match res {
            Ok(event) =>{
                match event.kind {
                    notify::event::EventKind::Create(file)=>{
                        // check if the created file is a not a directory
                        match file {
                            notify::event::CreateKind::File=>{
                                if let Err(err) = new_file_created(event,path.clone(),file_types.clone()) {
                                    log::error!("Error: {err:?}");
                            }},
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

fn new_file_created(event:notify::event::Event,path: Vec<String>,file_types: HashMap<String,String>) -> notify::Result<()> {
    println!("New file created: {:?}",event);
    // check if that file's parent directory is in the list of directories to watch
    let parent_dir = event.paths[0].parent().unwrap().to_str().unwrap();
    let mut parent_dir_in_list = false;
    for dir in path.iter() {
        if dir == parent_dir {
            parent_dir_in_list = true;
            break;
        }
    }
    if !parent_dir_in_list {
        return Ok(());
    }
    // check if the file type is in the list of file types to watch
    let file_type = event.paths[0].extension().unwrap().to_str().unwrap();
    let mut is_file_type_in_list = false;
    for file_type_in_list in file_types.keys() {
        if file_type_in_list == file_type {
            is_file_type_in_list = true;
            break;
        }
    }
    if !is_file_type_in_list {
        return Ok(());
    }
    // move the file to the directory specified in the config file
    let file_type = file_types.get(file_type).unwrap();
    let move_to_dir = Path::new(parent_dir).join(file_type);
    if !move_to_dir.exists() {
        fs::create_dir_all(move_to_dir.clone()).unwrap();
    }
    let file_name = event.paths[0].file_name().unwrap();
    let move_to_dir = move_to_dir.join(file_name);
    fs::rename(event.paths[0].clone(),move_to_dir.clone()).unwrap();
    Ok(())
}

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
