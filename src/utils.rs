/*
    Copyright 2021 Volt Contributors

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

// Std Imports
use std::path::Path;
use std::process;
use std::{borrow::Cow, env, path::PathBuf};
use std::{env::temp_dir, fs::File};
use std::{
    fs::create_dir_all,
    io::{self, Write},
};

// Library Imports
use anyhow::{Context, Result};
use colored::Colorize;
use dirs::home_dir;
use flate2::read::GzDecoder;
use fs_extra::dir::copy;
use sha1::{Digest, Sha1};
use tar::Archive;
use tokio::fs::remove_dir_all;

// Crate Level Imports
use crate::classes::voltapi::{VoltPackage, VoltResponse};

#[cfg(windows)]
pub static PROGRESS_CHARS: &str = "=> ";

#[cfg(unix)]
pub static PROGRESS_CHARS: &str = "▰▰▱";

lazy_static! {
    pub static ref ERROR_TAG: String = "error".red().bold().to_string();
}

pub struct App {
    pub current_dir: PathBuf,
    pub home_dir: PathBuf,
    pub node_modules_dir: PathBuf,
    pub volt_dir: PathBuf,
    pub lock_file_path: PathBuf,
    pub args: Vec<String>,
    pub flags: Vec<String>,
}

impl App {
    pub fn initialize() -> Self {
        enable_ansi_support().unwrap();

        let current_dir = env::current_dir().unwrap();
        let home_dir = home_dir().unwrap_or_else(|| current_dir.clone());
        let node_modules_dir = current_dir.join("node_modules");
        let volt_dir = home_dir.join(".volt");
        std::fs::create_dir_all(&volt_dir).ok();

        let lock_file_path = current_dir.join("volt.lock");

        let cli_args: Vec<_> = std::env::args().collect();
        let mut args: Vec<String> = Vec::new();
        let mut flags: Vec<String> = Vec::new();

        for arg in cli_args.into_iter().skip(2) {
            if arg.starts_with("--") || arg.starts_with("-") {
                flags.push(arg);
            } else {
                args.push(arg);
            }
        }

        App {
            args,
            flags,
            current_dir,
            home_dir,
            node_modules_dir,
            volt_dir,
            lock_file_path,
        }
    }

    pub fn has_flag(&self, flags: &[&str]) -> bool {
        self.flags
            .iter()
            .any(|flag| flags.iter().any(|search_flag| flag == search_flag))
    }

    pub async fn extract_tarball(&self, file_path: &str, package: &VoltPackage) -> Result<()> {
        // Open tar file
        let tar_file = File::open(file_path).context("Unable to open tar file")?;
        println!("{}", file_path);
        create_dir_all(&self.node_modules_dir)?;

        // Delete package from node_modules
        let node_modules_dep_path = self.node_modules_dir.join(&package.name);

        if node_modules_dep_path.exists() {
            remove_dir_all(&node_modules_dep_path).await?;
        }

        let volt_dir_file_path = &self.volt_dir.join(
            package
                .name
                .replace("/", "__")
                .replace("@", "")
                .replace(".", "_"),
        );

        let loc = format!(r"{}\{}", &self.volt_dir.to_str().unwrap(), package.name);

        let path = Path::new(&loc);

        if !path.exists() {
            // Extract tar file
            let gz_decoder = GzDecoder::new(tar_file);
            let mut archive = Archive::new(gz_decoder);
            archive
                .unpack(&self.volt_dir)
                .context("Unable to unpack dependency")?;

            std::fs::rename(
                format!(r"{}\package", &self.volt_dir.to_str().unwrap()),
                format!(
                    r"{}\{}",
                    &self.volt_dir.to_str().unwrap(),
                    package
                        .name
                        .replace("/", "__")
                        .replace("@", "")
                        .replace(".", "_"),
                ),
            )
            .context("Failed to unpack dependency folder")
            .unwrap_or_else(|e| println!("{} {}", "error".bright_red(), e));

            if let Some(parent) = node_modules_dep_path.parent() {
                if !parent.exists() {
                    create_dir_all(&parent)?;
                }
            }

            // if package.dependencies != None {
            //     let dependencies = &package.dependencies;
            //     for dep in &dependencies.clone().unwrap() {
            //         let volt_dir_symlink_path = &self.volt_dir.join(format!(
            //             r"{}\node_modules\{}",
            //             package
            //                 .name
            //                 .replace("/", "__")
            //                 .replace("@", "")
            //                 .replace(".", "_"),
            //             dep.replace("/", "__").replace("@", "").replace(".", "_"),
            //         ));

            //         let volt_dir_dep_path = &self
            //             .volt_dir
            //             .join(dep.replace("/", "__").replace("@", "").replace(".", "_"));

            //         if !volt_dir_dep_path.exists() {
            //             println!("unpacking dep: {}", dep);
            //             archive
            //                 .unpack(&self.volt_dir)
            //                 .context("Unable to unpack dependency")?;

            //             std::fs::rename(
            //                 format!(r"{}\package", &self.volt_dir.to_str().unwrap()),
            //                 format!(
            //                     r"{}\{}",
            //                     &self.volt_dir.to_str().unwrap(),
            //                     package
            //                         .name
            //                         .replace("/", "__")
            //                         .replace("@", "")
            //                         .replace(".", "_"),
            //                 ),
            //             )
            //             .context("Failed to unpack dependency folder")
            //             .unwrap_or_else(|e| println!("{} {}", "error".bright_red(), e));
            //         }

            //         // create_dir_all(volt_dir_symlink_path)?;
            //         // copy(volt_dir_dep_path, volt_dir_symlink_path, &options)?;
            // }
            // }

            create_symlink(
                volt_dir_file_path.as_os_str().to_str().unwrap().to_string(),
                node_modules_dep_path
                    .as_os_str()
                    .to_str()
                    .unwrap()
                    .to_string(),
            )?;
        }

        Ok(())
    }

    pub fn create_dep_symlinks() {}

    pub fn calc_hash<P: AsRef<Path>>(path: P) -> Result<String> {
        let mut file = File::open(path)?;

        let mut hasher = Sha1::new();
        io::copy(&mut file, &mut hasher)?;

        Ok(format!("{:x}", hasher.finalize()))
    }
}

// Gets response from volt CDN
pub async fn get_volt_response(package_name: String) -> VoltResponse {
    let response = reqwest::get(format!("http://volt-api.b-cdn.net/{}.json", package_name))
        .await
        .unwrap_or_else(|e| {
            println!("{} {}", "error".bright_red(), e);
            std::process::exit(1);
        })
        .text()
        .await
        .unwrap_or_else(|e| {
            println!("{} {}", "error".bright_red(), e);
            std::process::exit(1);
        });

    let data = serde_json::from_str::<VoltResponse>(&response).unwrap_or_else(|e| {
        println!("{} {}", "error".bright_red(), e);
        std::process::exit(1);
    });

    data
}

/// downloads tarball file from package
pub async fn download_tarball(_app: &App, package: &VoltPackage) -> Result<String> {
    let name = &package
        .name
        .replace("/", "__")
        .replace("@", "")
        .replace(".", "_");
    let file_name = format!("{}@{}.tgz", name, package.version);
    let temp_dir = temp_dir();
    if !Path::new(&temp_dir.join(r"\volt")).exists() {
        std::fs::create_dir(Path::new(&temp_dir.join(r"\volt")))?;
    }
    let path = temp_dir.join(format!(r"\volt\{}", file_name));
    let path_str = path.to_string_lossy().to_string();

    // Corrupt tar files may cause issues
    if let Ok(hash) = App::calc_hash(&path) {
        // File exists, make sure it's not corrupted
        if hash == package.sha1 {
            return Ok(path_str);
        }
    }

    let tarball = package.tarball.replace("https", "http");

    let mut response = reqwest::get(tarball).await?;

    // Placeholder buffer
    let mut file = File::create(&path)?;

    while let Some(chunk) = response.chunk().await? {
        file.write(&*chunk)?;
    }

    App::calc_hash(&path)?;

    Ok(path_str)
}

pub fn get_basename<'a>(path: &'a str) -> Cow<'a, str> {
    let sep: char;
    if cfg!(windows) {
        sep = '\\';
    } else {
        sep = '/';
    }
    let mut pieces = path.rsplit(sep);

    match pieces.next() {
        Some(p) => p.into(),
        None => path.into(),
    }
}

/// Gets a config key from git using the git cli.
pub fn get_git_config(key: &str) -> io::Result<Option<String>> {
    process::Command::new("git")
        .arg("config")
        .arg("--get")
        .arg(key)
        .output()
        .map(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout[..output.stdout.len() - 1].to_vec()).ok()
            } else {
                None
            }
        })
}

// Windows Function
#[cfg(windows)]
fn enable_ansi_support() -> Result<(), u32> {
    // ref: https://docs.microsoft.com/en-us/windows/console/console-virtual-terminal-sequences#EXAMPLE_OF_ENABLING_VIRTUAL_TERMINAL_PROCESSING @@ https://archive.is/L7wRJ#76%

    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use winapi::um::consoleapi::{GetConsoleMode, SetConsoleMode};
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::winnt::{FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE};

    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

    unsafe {
        // ref: https://docs.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew
        // Using `CreateFileW("CONOUT$", ...)` to retrieve the console handle works correctly even if STDOUT and/or STDERR are redirected
        let console_out_name: Vec<u16> =
            OsStr::new("CONOUT$").encode_wide().chain(once(0)).collect();
        let console_handle = CreateFileW(
            console_out_name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            0,
            null_mut(),
        );
        if console_handle == INVALID_HANDLE_VALUE {
            return Err(GetLastError());
        }

        // ref: https://docs.microsoft.com/en-us/windows/console/getconsolemode
        let mut console_mode: u32 = 0;
        if 0 == GetConsoleMode(console_handle, &mut console_mode) {
            return Err(GetLastError());
        }

        // VT processing not already enabled?
        if console_mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING == 0 {
            // https://docs.microsoft.com/en-us/windows/console/setconsolemode
            if 0 == SetConsoleMode(
                console_handle,
                console_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING,
            ) {
                return Err(GetLastError());
            }
        }
    }

    return Ok(());
}

/// Create a junction / hard symlink to a directory
#[cfg(windows)]
pub fn create_symlink(original: String, link: String) -> Result<()> {
    use crate::junction::lib as junction;
    junction::create(original, link)?;
    Ok(())
}

// Unix functions
#[cfg(unix)]
pub fn enable_ansi_support() -> Result<(), u32> {
    Ok(())
}

/// Create a symlink to a directory
#[cfg(unix)]
pub fn create_symlink<P: AsRef<Path>, Q: AsRef<Path>>(original: P, link: Q) -> Result<()> {
    std::os::unix::fs::symlink(original, link).context("Unable to symlink directory")
}

#[cfg(windows)]
pub fn generate_windows_binary(package: &VoltPackage) {
    if package.bin.is_some() {
        let bin = package.clone().bin.unwrap();
        let k = bin.keys().next().unwrap();
        let v = bin.values().next().unwrap();

        let command = format!(
            r#"
@IF EXIST "%~dp0\node.exe" (
    "%~dp0\node.exe"  "%~dp0\..\{}\{}" %*
) ELSE (
    @SETLOCAL
    @SET PATHEXT=%PATHEXT:;.JS;=;%
    node  "%~dp0\..\{}\{}" %*
)"#,
            k, v, k, v
        );
        println!("{}", command);

        let mut file = File::create(format!(r"node_modules\.bin\{}.cmd", k)).unwrap();
        //TODO: Move bin file to bin directory
    }
}
