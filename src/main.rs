use anyhow::{bail, Result};
use serde::Deserialize;
use std::fs;
use std::os::unix;
use std::path::PathBuf;
use thiserror::Error;

fn main() -> Result<()> {
    if cfg!(windows) {
        bail!("Windows is not supported.");
    }

    let symlinks_yaml = include_str!("symlinks.yml");
    // TODO: Look for custom config
    // TODO: provide option to dump the default config
    let config: Config = serde_yaml::from_str(symlinks_yaml)?;
    let source_dir: String = shellexpand::full(&config.source_dir)?.into();
    let source_dir =
        fs::canonicalize(&source_dir).map_err(|_| Error::MissingDirectory(source_dir))?;

    if !source_dir.exists() {
        bail!(
            "Source directory '{}' does not exist.",
            source_dir.display()
        );
    }

    for Link { src, dest } in config.links {
        let src = source_dir.join(src);
        if !src.exists() {
            println!("Path '{}' does not exist. Skipping...", src.display());
            continue;
        }
        let dest: String = shellexpand::full(&dest)?.into();
        let dest: PathBuf = dest.into();
        let dest_parent = match dest.parent() {
            Some(path) => path,
            None => {
                // This should only happen if dest is '/'.
                println!("Cannot link to '{}'. Skipping...", dest.display());
                continue;
            }
        };
        let dest_parent = match fs::canonicalize(&dest_parent) {
            Ok(path) => path,
            Err(_) => {
                println!(
                    "Cannot link to '{}' because its parent directory does not exist. Skipping...",
                    dest.display()
                );
                continue;
            }
        };
        let dest_file_name = match dest.file_name() {
            Some(file_name) => file_name,
            None => {
                println!("Invalid destination path '{}'. Skipping...", dest.display());
                continue;
            }
        };
        let dest = dest_parent.join(dest_file_name);

        if dest.exists() {
            let mut backup_file = dest_file_name.to_owned();
            backup_file.push(
                chrono::Local::now()
                    .format("-backup-%Y-%m-%d-%H-%M-%S")
                    .to_string(),
            );
            let backup = dest_parent.join(backup_file);
            print!(
                "The path '{}' already exists. Backing up to '{}'...",
                dest.display(),
                backup.display()
            );
            match fs::rename(&dest, backup) {
                Ok(()) => println!("done."),
                Err(_) => println!("\nBackup failed. Skipping..."),
            }
        }
        print!("Linking {} -> {}...", src.display(), dest.display());
        match unix::fs::symlink(&src, &dest) {
            Ok(()) => println!("done."),
            Err(_) => println!(
                "\nFailed to symlink {} -> {}. Skipping...",
                src.display(),
                dest.display()
            ),
        };
    }

    Ok(())
}

#[derive(Deserialize, Debug)]
struct Config {
    source_dir: String,
    links: Vec<Link>,
}

#[derive(Deserialize, Debug)]
struct Link {
    src: String,
    dest: String,
}

#[derive(Error, Debug)]
enum Error {
    #[error("Directory '{0}' does not exist")]
    MissingDirectory(String),
}
