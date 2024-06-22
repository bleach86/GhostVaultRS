use home;
use log::{error, info};
use serde_json::{json, Error as json_Error, Map, Value};
use std::fs;
use std::{
    env::temp_dir,
    fs::File,
    io::{prelude::*, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

pub fn rm_dir(path: &PathBuf) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    fs::remove_dir_all(path)?;
    info!("Directory '{}' removed", path.to_string_lossy());
    Ok(())
}

pub fn rm_file(path: &PathBuf) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(path)?;
    info!("File '{}' removed", path.to_string_lossy());
    Ok(())
}

pub fn create_dir(dir: &PathBuf) -> std::io::Result<()> {
    if dir.exists() {
        return Ok(());
    }

    fs::create_dir_all(dir)?;
    Ok(())
}

pub fn rename_file(file: &PathBuf, old_name: &str, new_name: &str) -> std::io::Result<()> {
    let old_file_path: PathBuf = file.join(old_name);
    let new_file_path: PathBuf = file.join(new_name);

    fs::rename(old_file_path, new_file_path)?;

    Ok(())
}

pub fn init_data_dir(path: &PathBuf) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    let daemon_dir: PathBuf = PathBuf::from("daemon/");

    create_dir(path)?;
    create_dir(&path.join(daemon_dir))?;

    Ok(())
}

pub fn read_json(path: &PathBuf) -> Result<Value, json_Error> {
    let file_str: String = fs::read_to_string(path).unwrap_or(json!({}).to_string());
    let json_data: Value = serde_json::from_str(&file_str.as_str())?;

    Ok(json_data)
}

pub fn get_pid(conf_path: &PathBuf, pid_file: &str) -> u32 {
    let pid_file = conf_path.join(pid_file);

    if !pid_file.exists() {
        return 0;
    }

    let mut file_str: String = fs::read_to_string(pid_file).unwrap_or_default();

    remove_whitespace(&mut file_str);
    file_str.parse::<u32>().unwrap()
}

pub fn make_pid_file(conf_path: &PathBuf, pid_file: &str) -> Result<(), String> {
    let pid_file: PathBuf = conf_path.join(pid_file);
    let pid: u32 = std::process::id();

    if let Err(err) = fs::write(pid_file, pid.to_string().as_bytes()) {
        return Err(format!("Error writing to file: {}", err));
    }

    Ok(())
}

pub fn update_ghost_config(
    path: &PathBuf,
    config_key: &str,
    config_value: Option<&str>,
) -> Result<Value, json_Error> {
    // Load the JSON configuration from file
    let mut ghost_conf_value: Value = ghost_config_to_value(path)?;

    if let None = config_value {
        if let Some(obj) = ghost_conf_value.as_object_mut() {
            obj.remove(config_key);
        }
    } else {
        let value_str: &str = config_value.unwrap();
        let new_value: Value = match value_str.parse::<u64>() {
            Ok(value) => Value::Number(serde_json::Number::from(value)),
            Err(_) => Value::String(value_str.to_string()),
        };

        // Check if the key already exists in the JSON object
        if let Some(existing_value) = ghost_conf_value.get_mut(config_key) {
            // If it exists, update the value
            *existing_value = new_value;
        } else {
            // If it doesn't exist, insert a new key-value pair
            let json_object: &mut Map<String, Value> = ghost_conf_value
                .as_object_mut()
                .expect("Expected an object here");
            json_object.insert(config_key.to_string(), new_value);
        }
    }

    save_ghost_config(path, &ghost_conf_value).unwrap();

    Ok(ghost_conf_value)
}

pub fn ghost_config_to_value(path: &PathBuf) -> Result<Value, json_Error> {
    let file_vec: Vec<String> = lines_from_file(path);
    let mut json_object: Map<String, Value> = Map::new();

    for line in file_vec.into_iter() {
        if line.is_empty() || !line.contains("=") {
            continue;
        }
        let split_line: Vec<&str> = line.split("=").collect::<Vec<&str>>();
        let config_key: &str = split_line[0];
        let config_value: &str = split_line[1];

        let value = match config_value.parse::<u64>() {
            Ok(value) => Value::Number(serde_json::Number::from(value)),
            Err(_) => Value::String(config_value.to_string()),
        };

        json_object.insert(config_key.to_string(), value);
    }

    let json_value: Value = Value::Object(json_object);

    Ok(json_value)
}

fn lines_from_file(filename: &PathBuf) -> Vec<String> {
    let file_res: Result<File, std::io::Error> = File::open(filename);

    let file = match file_res {
        Ok(file) => file,
        Err(_) => return Vec::new(),
    };

    let buf: BufReader<File> = BufReader::new(file);
    buf.lines()
        .map(|l: Result<String, std::io::Error>| l.unwrap_or_default())
        .collect()
}

fn remove_whitespace(s: &mut String) {
    s.retain(|c| !c.is_whitespace());
}

fn save_ghost_config(path: &PathBuf, config: &Value) -> Result<(), String> {
    let mut ghost_conf_str: String = String::new();

    // convert our Value Object to the format ghostd is expecting
    if let Some(obj) = config.as_object() {
        for (key, value) in obj {
            ghost_conf_str.push_str(format!("{}={}\n", key, value.as_str().unwrap()).as_str());
        }
    }

    if let Err(err) = fs::write(path, ghost_conf_str.as_bytes()) {
        return Err(format!("Error writing to file: {}", err));
    }

    Ok(())
}

pub fn expand_user(dir: &str) -> PathBuf {
    if dir.starts_with("~/") {
        let home_dir: PathBuf = home::home_dir().unwrap();
        home_dir.join(dir.strip_prefix("~/").unwrap())
    } else {
        PathBuf::from(dir)
    }
}

pub fn read_crontab() -> String {
    let username: String = whoami::username();

    let output = Command::new("crontab")
        .arg("-l")
        .arg("-u")
        .arg(username)
        .output();

    match output {
        Ok(output) => {
            if output.status.success() {
                String::from_utf8_lossy(&output.stdout).to_string()
            } else {
                error!("Error reading crontab");
                String::new()
            }
        }
        Err(_) => {
            error!("Error reading crontab");
            String::new()
        }
    }
}

pub fn remove_legacy_cron_entry(current_crontab: &str) -> String {
    let legacy_cmd: &str = "/usr/bin/python3 ghostVault.py";
    let mut mod_tab: Vec<String> = Vec::new();

    for line in current_crontab.split("\n") {
        let sanitized_line: String = if line.contains(legacy_cmd) && !line.starts_with("#") {
            let mod_line: String = format!("#{}", line);
            mod_line
        } else {
            line.to_string()
        };

        mod_tab.push(sanitized_line);
    }

    let modified_crontab: String = format!("{}\n", mod_tab.join("\n"));

    modified_crontab
}

pub fn write_crontab(modified_crontab: &str) -> std::io::Result<()> {
    let username: String = whoami::username();
    let modified_crontab = if modified_crontab.ends_with('\n') {
        modified_crontab.to_owned()
    } else {
        format!("{}\n", modified_crontab)
    };

    let temp_file_path: PathBuf = temp_dir().join("modified_crontab.txt");
    let mut temp_file: File = File::create(&temp_file_path)?;

    temp_file.write_all(modified_crontab.as_bytes())?;

    let status = Command::new("crontab")
        .arg("-u")
        .arg(username)
        .arg(&temp_file_path)
        .status();

    match status {
        Ok(status) => {
            if status.success() {
                info!("Successfully wrote crontab");
            } else {
                error!("Error writing crontab");
            }
        }
        Err(_) => error!("Error writing crontab"),
    }

    std::fs::remove_file(&temp_file_path)?;

    Ok(())
}

pub fn is_crontab_installed() -> bool {
    let status = Command::new("crontab")
        .arg("-l") // Check the version to see if crontab is installed
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

pub fn pid_exists(pid: u32) -> bool {
    // It's possible for pid 0 to exist,
    // but it will never be what we are looking for.
    if pid == 0 {
        return false;
    }
    PathBuf::from(&format!("/proc/{pid}")).exists()
}
