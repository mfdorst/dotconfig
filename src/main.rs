use anyhow::{bail, Result};
use clap::{load_yaml, App, ArgMatches};
use serde::Deserialize;
use std::{
    ffi::OsStr,
    fs::File,
    fs::{self, read_link},
    io::BufReader,
    os::unix,
    path::PathBuf,
};
use thiserror::Error;

fn main() -> Result<()> {
    if cfg!(windows) {
        bail!("Windows is not supported.");
    }
    let yaml = load_yaml!("cli.yml");
    let args = App::from_yaml(yaml).get_matches();
    let (config, dotfiles_dir) = get_config(&args)?;

    // Symlink each file listed in config.links
    for Link { src, dest } in config.links {
        if let Err(e) = link(&src, &dest, &dotfiles_dir) {
            println!("{}", e);
        }
    }
    Ok(())
}

macro_rules! link_error {
    ($fmt:expr, $($arg:tt)*) => { anyhow::Error::from(Error::LinkError(format!($fmt, $($arg)*))) }
}

fn link(src: &str, dest: &str, dotfiles_dir: &PathBuf) -> Result<()> {
    let src = get_src_path(dotfiles_dir, src)?;
    let dest = expand_to_path_buf(&dest)?;

    let dest_parent = get_parent_dir(&dest).map_err(|_| {
        link_error!(
            "Cannot link to '{}' because its parent directory does not exist. Skipping...",
            dest.display()
        )
    })?;
    let dest_file_name = dest.file_name().ok_or(link_error!(
        "Invalid destination path '{}'. Skipping...",
        dest.display()
    ))?;

    if dest.exists() {
        if let Ok(existing_link_src) = read_link(&dest) {
            if fs::canonicalize(&src)? == fs::canonicalize(&existing_link_src)? {
                println!(
                    "Skipping '{}' -> '{}'. File already linked.",
                    src.display(),
                    dest.display()
                );
                return Ok(());
            } else {
                println!(
                    "The path '{}' is already symlinked to '{}'.",
                    dest.display(),
                    existing_link_src.display(),
                );
                backup(&dest_parent, dest_file_name)?;
            }
        } else {
            backup(&dest_parent, dest_file_name)?;
        }
    }

    let dest = dest_parent.join(dest_file_name);

    print!("Linking '{}' -> '{}'...", dest.display(), src.display());
    unix::fs::symlink(&src, &dest)
        .map(|_| println!("done."))
        .map_err(|e| {
            link_error!(
                "\nFailed to symlink {} -> {}. {}. Skipping...",
                src.display(),
                dest.display(),
                e
            )
        })
}

fn get_src_path(dotfiles_dir: &PathBuf, src: &str) -> Result<PathBuf> {
    let src = dotfiles_dir.join(src);
    let src = fs::canonicalize(&src)
        .map_err(|_| link_error!("Path '{}' does not exist. Skipping...", src.display()))?;
    Ok(src)
}

fn backup(parent_dir: &PathBuf, file_name: &OsStr) -> Result<()> {
    let path = parent_dir.join(file_name);
    let mut backup_file = file_name.to_owned();
    backup_file.push(
        chrono::Local::now()
            .format("-backup-%Y-%m-%d-%H-%M-%S")
            .to_string(),
    );
    let backup = parent_dir.join(backup_file);
    print!(
        "The path '{}' already exists. Backing up to '{}'...",
        path.display(),
        backup.display()
    );
    fs::rename(&path, backup)
        .map(|_| println!("done."))
        .map_err(|e| link_error!("\nBackup failed. {}", e))
}

fn get_parent_dir(path: &PathBuf) -> Result<PathBuf> {
    if let Some(parent_dir) = path.parent() {
        fs::canonicalize(&parent_dir).map_err(|e| e.into())
    } else {
        bail!("Cannot get parent of '{}'.", path.display())
    }
}

fn get_config(args: &ArgMatches) -> Result<(Config, PathBuf)> {
    // It's okay to unwrap here because dir has a default argument and will never be None.
    let dotfiles_dir = expand_to_path_buf(args.value_of("dir").unwrap())?;
    // It's okay to unwrap here because config has a default argument and will never be None
    let config_rel_path = expand_to_path_buf(args.value_of("config").unwrap())?;
    let config_full_path = dotfiles_dir.join(config_rel_path);

    if !dotfiles_dir.exists() {
        return Err(Error::MissingDotfilesDir(dotfiles_dir).into());
    }
    if !config_full_path.exists() {
        return Err(Error::MissingConfigFile(config_full_path).into());
    }
    let reader = BufReader::new(File::open(config_full_path)?);
    let config: Config = serde_yaml::from_reader(reader)?;
    Ok((config, dotfiles_dir))
}

fn expand_to_path_buf(path: &str) -> Result<PathBuf> {
    Ok(shellexpand::full(path)?.to_string().into())
}

#[derive(Deserialize, Debug)]
struct Config {
    links: Vec<Link>,
}

#[derive(Deserialize, Debug)]
struct Link {
    src: String,
    dest: String,
}

#[derive(Error, Debug)]
enum Error {
    #[error("Missing dotfiles directory ({0}).")]
    MissingDotfilesDir(PathBuf),
    #[error("Missing config file ({0}).")]
    MissingConfigFile(PathBuf),
    #[error("{0}")]
    LinkError(String),
}
