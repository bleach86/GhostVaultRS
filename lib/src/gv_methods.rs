#![allow(dead_code)]
use crate::{
    constants::{DAEMON_BASE_URL, LATEST_RELEASE_URL, TMP_PATH},
    file_ops,
};
use data_encoding::HEXLOWER;
use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use log::{error, info};
use reqwest::{header::CONTENT_LENGTH, Client, Response};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
};
use tar::Archive;
use tokio::io::AsyncWriteExt;
use walkdir::WalkDir;

use futures::future::select_ok;
use futures_util::FutureExt;

pub struct PathAndDigest {
    pub daemon_path: PathBuf,
    pub daemon_hash: String,
}

pub async fn download_file(
    url: &str,
    file_name: &str,
    with_progress: bool,
) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    // Create a progress bar
    let mut progress_bar: Option<ProgressBar> = if with_progress {
        let pb = ProgressBar::new(0);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("Downloading ghostd {bar:30.cyan/blue} {percent}% {bytes}/{total_bytes} ({eta})")
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    // Create a reqwest client
    let client: Client = Client::new();

    // Send HTTP GET request
    let mut response: Result<Response, reqwest::Error> = client.get(url).send().await;

    // Retry the request if it fails
    let mut retries: u8 = 0;
    while response.is_err() && retries < 3 {
        error!(
            "Failed to download file, retrying\nError: {}",
            response.err().unwrap()
        );
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        retries += 1;
        response = client.get(url).send().await;
    }

    // Check if the request failed
    if response.is_err() {
        let err_msg = response.err().unwrap();
        error!("Failed to download file: {}", err_msg);
        return Err(format!("Failed to download file: {}", err_msg).into());
    }

    let mut response: Response = response.unwrap();

    // Check if the request was successful
    if !response.status().is_success() {
        return Err(format!(
            "Failed to download file, status code: {}",
            response.status()
        )
        .into());
    }

    // Get the content length to determine the total file size
    let total_size: u64 = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v: &hyper::header::HeaderValue| v.to_str().ok())
        .and_then(|s: &str| s.parse().ok())
        .unwrap_or(0);

    // Create or open the destination file
    let mut file: tokio::fs::File = tokio::fs::File::create(file_name).await?;

    // Stream the response and write to the file with progress
    let mut downloaded_size: u64 = 0;
    while let Some(chunk) = response.chunk().await? {
        let chunk_size: u64 = chunk.len() as u64;
        downloaded_size += chunk_size;

        if let Some(pb) = &mut progress_bar {
            // Update the progress bar
            pb.set_position(downloaded_size);
            pb.set_length(total_size);
        }

        // Write the chunk to the file
        file.write_all(&chunk).await?;
    }

    if let Some(pb) = &mut progress_bar {
        // Finish the progress bar
        pb.finish_with_message("Download complete");
    }

    let full_path: PathBuf = PathBuf::from(&file_name).canonicalize()?;

    Ok(full_path)
}

pub async fn get_latest_release() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let client: Client = Client::new();
    let response: Result<Response, reqwest::Error> = client.get(LATEST_RELEASE_URL).send().await;

    if response.is_err() {
        return Err(format!("Failed to get latest release: {}", response.err().unwrap()).into());
    }

    let response: Response = response.unwrap();

    let final_url: String = response.url().to_string();
    let version: String = final_url
        .split('/')
        .last()
        .unwrap_or_default()
        .strip_prefix("v")
        .unwrap_or_default()
        .to_string();

    Ok(version)
}

pub async fn download_daemon() -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let latest_version: String = get_latest_release().await?;

    let tripple: String = get_tripple();

    let download_url: String = format!(
        "{}v{}/ghost-{}-{}.tar.gz",
        DAEMON_BASE_URL, latest_version, latest_version, tripple
    );

    let file_name: String = format!("ghost-{}-{}.tar.gz", latest_version, tripple);

    // download the hashes.txt file

    let file_name_hashes: String = "hashes.txt".to_string();
    let hashes_url: String = format!(
        "{}v{}/{}",
        DAEMON_BASE_URL, latest_version, file_name_hashes
    );

    let tmp_path: PathBuf = PathBuf::from(TMP_PATH);

    if !tmp_path.exists() {
        file_ops::create_dir(&tmp_path)?;
    }

    let file_name_hashes_vers: String = format!(
        "{}/v{}-hashes.txt",
        tmp_path.to_string_lossy(),
        latest_version
    );

    let file_name_hashes_buff: PathBuf = PathBuf::from(&file_name_hashes_vers);

    let dl_hashes: PathBuf = if !file_name_hashes_buff.exists() {
        let dl_hash_path: PathBuf =
            download_file(&hashes_url, &file_name_hashes_vers, false).await?;
        dl_hash_path
    } else {
        file_name_hashes_buff
    };

    if !dl_hashes.exists() {
        error!("Failed to download hashes")
    }

    let file_path: PathBuf =
        PathBuf::from(format!("{}/{}", tmp_path.to_string_lossy(), &file_name));

    if file_path.exists() {
        // if the file already exists
        // compare the hashes and don't donwload again if they match

        if compare_digest_daemon(&file_path, &dl_hashes)? {
            return Ok(file_path);
        }
    }

    let download_path = download_file(
        download_url.as_str(),
        file_path.as_os_str().to_str().unwrap(),
        true,
    )
    .await?;

    Ok(download_path)
}

pub fn extract_archive(
    archive_path: &PathBuf,
    gv_home_dir: &PathBuf,
) -> Result<PathAndDigest, Box<dyn std::error::Error + Send + Sync>> {
    info!("Extracting Ghost daemon...");
    let daemon_dir: PathBuf = gv_home_dir.join("daemon/");

    let tar_gz: File = File::open(archive_path)?;
    let tar: GzDecoder<File> = GzDecoder::new(tar_gz);
    let mut archive: Archive<GzDecoder<File>> = Archive::new(tar);
    archive.unpack(&daemon_dir)?;

    // we walk the download path to find ghostd.
    // this is to prevent issues if ghostd is not packaged as expected.

    let mut daemon_path: Option<PathBuf> = None;

    for entry in WalkDir::new(&daemon_dir) {
        if let Ok(entry) = entry {
            if let Some(filename) = entry.file_name().to_str() {
                let is_windows = cfg!(target_os = "windows");

                if is_windows && filename == "ghostd.exe" {
                    daemon_path = Some(entry.path().to_owned());
                    break;
                } else if !is_windows && filename == "ghostd" {
                    daemon_path = Some(entry.path().to_owned());
                    break;
                }
            }
        }
    }

    if let Some(path) = daemon_path {
        let daemon_path: PathBuf = path.canonicalize()?;
        let daemon_hash: String = sha256_digest(&path)?;

        let path_and_digest: PathAndDigest = PathAndDigest {
            daemon_path,
            daemon_hash,
        };

        Ok(path_and_digest)
    } else {
        panic!("Daemon not found");
    }
}

/// calculates sha256 digest as lowercase hex string
pub fn sha256_digest(path: &PathBuf) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let input: File = File::open(path)?;
    let mut reader: BufReader<File> = BufReader::new(input);

    let digest = {
        let mut hasher = Sha256::new();
        let mut buffer: [u8; 1024] = [0; 1024];
        loop {
            let count: usize = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        hasher.finalize()
    };
    Ok(HEXLOWER.encode(digest.as_ref()))
}

fn get_tripple() -> String {
    let arch: &str = env::consts::ARCH;
    let os: &str = env::consts::OS;

    let tripple: &str = match (arch, os) {
        ("x86_64", "linux") => "pc-linux-gnu",
        ("arm", "linux") => "linux-gnueabihf",
        ("aarch64", "linux") => "linux-gnu",
        ("x86_64", "macos") => "MacOS64",
        ("aarch64", "macos") => "MacOS64",
        ("x86_64", "windows") => "win64",
        _ => panic!("OS or CPU Arch not supported."),
    };

    format!("{arch}-{tripple}")
}

fn compare_digest_daemon(
    file_path: &PathBuf,
    hashes_path: &PathBuf,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let file_hash: String = sha256_digest(&file_path)?;

    let hashes_file: File = File::open(&hashes_path)?;
    let reader: BufReader<File> = BufReader::new(hashes_file);
    let data: Vec<String> = reader
        .lines()
        .map(|l| l.expect("Could not parse line"))
        .collect();

    for line in data.iter() {
        let split_line: Vec<&str> = line.split_whitespace().collect();

        if split_line.last().unwrap() == &file_path.file_name().unwrap().to_str().unwrap() {
            if split_line.first().unwrap() == &file_hash.as_str() {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

pub async fn get_remote_best_block() -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let node: Vec<String> = get_remote_nodes();

    let url1: String = format!("{}/getblockcount/", node[0]);
    let url2: String = format!("{}/getblockcount/", node[1]);
    let url3: String = format!("{}/getblockcount/", node[2]);
    let url4: String = format!("{}/getblockcount/", node[3]);

    let result = select_ok(vec![
        make_get_req(url1).boxed(),
        make_get_req(url2).boxed(),
        make_get_req(url3).boxed(),
        make_get_req(url4).boxed(),
    ])
    .await?;

    Ok(result.0)
}

pub async fn get_remote_block_hash(
    block_index: u32,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let node: Vec<String> = get_remote_nodes();

    let url1: String = format!("{}/api/block-index/{}/", node[0], block_index);
    let url2: String = format!("{}/api/block-index/{}/", node[1], block_index);
    let url3: String = format!("{}/api/block-index/{}/", node[2], block_index);
    let url4: String = format!("{}/api/block-index/{}/", node[3], block_index);

    let result = select_ok(vec![
        make_get_req(url1).boxed(),
        make_get_req(url2).boxed(),
        make_get_req(url3).boxed(),
        make_get_req(url4).boxed(),
    ])
    .await?;

    Ok(result.0)
}

pub async fn get_remote_block_chain_info() -> Result<Value, Box<dyn std::error::Error + Send + Sync>>
{
    let node: Vec<String> = get_remote_nodes();

    let url1: String = format!("{}/getblockchaininfo/", node[0]);
    let url2: String = format!("{}/getblockchaininfo/", node[1]);
    let url3: String = format!("{}/getblockchaininfo/", node[2]);
    let url4: String = format!("{}/getblockchaininfo/", node[3]);

    let result = select_ok(vec![
        make_get_req(url1).boxed(),
        make_get_req(url2).boxed(),
        make_get_req(url3).boxed(),
        make_get_req(url4).boxed(),
    ])
    .await?;

    Ok(result.0)
}

async fn make_get_req(url: String) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let res: Response = reqwest::get(url).await?;
    let json_data: Value = res.json().await?;

    Ok(json_data)
}

fn get_remote_nodes() -> Vec<String> {
    let nodes_init: Vec<&str> = vec![
        "https://api.tuxprint.com",
        "https://api2.tuxprint.com",
        "https://socket.tuxprint.com",
        "https://socket2.tuxprint.com",
    ];

    let mut nodes: Vec<String> = Vec::new();

    for node in nodes_init {
        nodes.push(node.to_string())
    }

    nodes
}

pub async fn validate_bot_token(
    token: &str,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let api_url = format!("https://api.telegram.org/bot{}/getMe", token);
    let response = reqwest::get(&api_url).await?;

    if response.status().is_success() {
        let data = response.json::<serde_json::Value>().await?;
        if data["ok"].as_bool().unwrap_or(false) {
            return Ok(true);
        } else {
            return Ok(false);
        }
    } else {
        return Ok(false);
    }
}
