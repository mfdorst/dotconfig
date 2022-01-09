use clap::Parser;
use serde::Deserialize;
use std::{
    ffi::OsStr,
    fs::{self, read_link, File},
    io::BufReader,
    os::unix,
    path::PathBuf,
};
use thiserror::Error;
use yansi::Paint;

type Result<T, E = Error> = std::result::Result<T, E>;

/// Symlinks configuration files from a central location to wherever they need to be on the system,
/// so that those config files can be maintained under version control.
#[derive(Parser, Debug)]
#[clap(about, author, version)]
pub struct Cli {
    /// Specify the directory that holds your config files
    #[clap(short, long, default_value = "$HOME/.cfg")]
    dir: String,
    /// Specify the YAML file that lists your desired symlinks
    #[clap(short, long, default_value = "symlinks.yml")]
    config: String,
}

fn main() -> Result<()> {
    if cfg!(windows) {
        return Err(Error::UnsupportedPlatform);
    }
    let cli = Cli::parse();
    let (config, dotfiles_dir) = get_config(&cli)?;

    // Symlink each file listed in config.links
    for Link { origin, path: link } in config.links {
        if let Err(e) = symlink(&origin, &link, &dotfiles_dir) {
            println!("{}", e);
        }
    }
    Ok(())
}

macro_rules! link_error {
    ($fmt:expr, $($arg:tt)*) => { Error::LinkError(format!($fmt, $($arg)*)) }
}

fn symlink(origin: &str, link: &str, dotfiles_dir: &PathBuf) -> Result<()> {
    let origin = get_origin_path(dotfiles_dir, origin)?;
    let link = PathBuf::from(shellexpand::full(link)?.into_owned());

    let link_parent = fs::canonicalize(&link).map_err(|_| {
        link_error!(
            "{} '{}' {}",
            Paint::red("Cannot create link"),
            link.display(),
            Paint::red("because the parent directory does not exist. Skipping...")
        )
    })?;
    let link_file_name = link.file_name().ok_or(link_error!(
        "{} '{}'. {}",
        Paint::red("Invalid path"),
        link.display(),
        Paint::red("Skipping...")
    ))?;

    if link.exists() {
        if let Ok(existing_link_origin) = read_link(&link) {
            if fs::canonicalize(&origin)? == fs::canonicalize(&existing_link_origin)? {
                println!(
                    "{} '{}' {} '{}'{}",
                    Paint::green("Skipping"),
                    origin.display(),
                    Paint::green("->"),
                    link.display(),
                    Paint::green(". File already linked.")
                );
                return Ok(());
            } else {
                print!(
                    "{} '{}' {} '{}'{} ",
                    Paint::yellow("The path"),
                    link.display(),
                    Paint::yellow("is already linked to"),
                    existing_link_origin.display(),
                    Paint::yellow(".")
                );
                backup(&link_parent, link_file_name)?;
            }
        } else {
            print!(
                "{} '{}' {} ",
                Paint::yellow("The path"),
                link.display(),
                Paint::yellow("already exists.")
            );
            backup(&link_parent, link_file_name)?;
        }
    }

    let link = link_parent.join(link_file_name);

    print!(
        "{} '{}' {} '{}'...",
        Paint::yellow("Linking"),
        link.display(),
        Paint::yellow("->"),
        origin.display()
    );
    unix::fs::symlink(&origin, &link)
        .map(|_| println!("{}", Paint::green("done.")))
        .map_err(|e| {
            link_error!(
                "\n{} {} -> {}. {}. {}",
                Paint::red("Failed to link"),
                origin.display(),
                link.display(),
                Paint::yellow(e),
                Paint::red("Skipping...")
            )
        })
}

fn get_origin_path(dotfiles_dir: &PathBuf, origin: &str) -> Result<PathBuf> {
    let origin = dotfiles_dir.join(origin);
    let origin = fs::canonicalize(&origin).map_err(|_| {
        link_error!(
            "{} '{}' {}",
            Paint::red("The path"),
            origin.display(),
            Paint::red("does not exist. Skipping...")
        )
    })?;
    Ok(origin)
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
        "{} '{}'...",
        Paint::yellow("Backing up to"),
        backup.display()
    );
    fs::rename(&path, backup)
        .map(|_| println!("{}", Paint::green("done.")))
        .map_err(|e| link_error!("{} {}", Paint::red("Backup failed."), Paint::yellow(e)))
}

fn get_config(cli: &Cli) -> Result<(Config, PathBuf)> {
    let dotfiles_dir = PathBuf::from(shellexpand::full(&cli.dir)?.into_owned());
    let config_rel_path = PathBuf::from(shellexpand::full(&cli.config)?.into_owned());
    let config_full_path = dotfiles_dir.join(config_rel_path);

    if !dotfiles_dir.exists() {
        return Err(Error::MissingDotfilesDir(dotfiles_dir));
    }
    if !config_full_path.exists() {
        return Err(Error::MissingConfigFile(config_full_path));
    }
    let reader = BufReader::new(File::open(config_full_path)?);
    let config: Config = serde_yaml::from_reader(reader)?;
    Ok((config, dotfiles_dir))
}

#[derive(Deserialize, Debug)]
struct Config {
    links: Vec<Link>,
}

#[derive(Deserialize, Debug)]
struct Link {
    path: String,
    origin: String,
}

#[derive(Error, Debug)]
enum Error {
    #[error("Missing dotfiles directory ({0}).")]
    MissingDotfilesDir(PathBuf),
    #[error("Missing config file ({0}).")]
    MissingConfigFile(PathBuf),
    #[error("{0}")]
    LinkError(String),
    #[error("Windows is not supported.")]
    UnsupportedPlatform,
    #[error("IoError: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Eror in YAML ({0})")]
    YamlError(#[from] serde_yaml::Error),
    #[error("Unknown variable ({0})")]
    ShellexpandLookupError(#[from] shellexpand::LookupError<std::env::VarError>),
}
