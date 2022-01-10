use clap::Parser;
use serde::Deserialize;
use std::{
    ffi::OsString,
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

    // Get the paths of the dotfiles directory and the symlink list
    let dotfiles_dir = PathBuf::from(shellexpand::full(&cli.dir)?.into_owned());
    let symlink_list_rel_path = PathBuf::from(shellexpand::full(&cli.config)?.into_owned());
    let symlink_list_full_path = dotfiles_dir.join(symlink_list_rel_path);

    if !dotfiles_dir.exists() {
        return Err(Error::MissingDotfilesDir(dotfiles_dir));
    }
    if !symlink_list_full_path.exists() {
        return Err(Error::MissingSymlinkListFile(symlink_list_full_path));
    }
    let reader = BufReader::new(File::open(symlink_list_full_path)?);
    let symlink_list: SymlinkList = serde_yaml::from_reader(reader)?;

    // Symlink each file listed in config.links
    for Link { origin, path: link } in symlink_list.links {
        if let Err(e) = symlink(&origin, &link, &dotfiles_dir) {
            println!("{}", e);
        }
    }
    Ok(())
}

/// Create a symlink from `link` to `origin`. If `origin` already exists, back it up (rename it to
/// `<filename>-backup-<date>`) first. If the symlink already exists, do nothing. If either `link`
/// or `origin` are invalid paths, do nothing.
///
/// # Params
/// + `link` - The path where the symlink will be created.
/// + `origin` - The path that the symlink will point to. Relative to `dotfiles_dir`.
/// + `dotfiles_dir` - The dotfiles directory that contains `origin`.
///
/// # Errors
/// + [`Error::LinkError`]
///     + If the path `link` does not exist. Either:
///         + the parent directory does not exist, or
///         + the path is invalid in some other way, such as not being relative to root (`/`).
///     + If the symlink failed for some other reason (probably a bug).
///     + If `origin` does not exist as a path within the `dotfiles_dir` directory.
fn symlink(origin: &str, link: &str, dotfiles_dir: &PathBuf) -> Result<()> {
    let (origin, link, link_parent, link_filename) = expand_paths(dotfiles_dir, link, origin)?;

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
                backup(&link_parent, link_filename)?;
            }
        } else {
            print!(
                "{} '{}' {} ",
                Paint::yellow("The path"),
                link.display(),
                Paint::yellow("already exists.")
            );
            backup(&link_parent, link_filename)?;
        }
    }

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
            Error::LinkError(format!(
                "\n{} {} -> {}. {}. {}",
                Paint::red("Failed to link"),
                origin.display(),
                link.display(),
                Paint::yellow(e),
                Paint::red("Skipping...")
            ))
        })
}

/// Returns the following paths (listed in tuple order) in canonical, absolute form with all
/// intermediate components normalized and symbolic links resolved. See [`fs::canonicalize`].
///
/// + The full path to the file that should be linked to
/// + The full path where the symlink should be created (including file name)
/// + The full path to the symlink's parent (the previous minus the filename)
/// + The symlink's filename
///
/// # Errors
/// + [Error::LinkError]
///     + If the the path `dotfiles_dir/origin` does not exist.
///     + If `link` is an invalid path.
///     + If the parent directory of `link` does not exist.
/// + [Error::ShellexpandLookupError] if a shell variable in `link` is not found in the current
/// environment.
fn expand_paths(
    dotfiles_dir: &PathBuf,
    link: &str,
    origin: &str,
) -> Result<(PathBuf, PathBuf, PathBuf, OsString)> {
    let origin = dotfiles_dir.join(origin);
    let origin = fs::canonicalize(&origin).map_err(|_| {
        Error::LinkError(format!(
            "{} '{}' {}",
            Paint::red("The path"),
            origin.display(),
            Paint::red("does not exist. Skipping...")
        ))
    })?;

    let link = PathBuf::from(shellexpand::full(link)?.into_owned());

    let link_parent = link.parent().ok_or(Error::LinkError(format!(
        "{} '{}' {}",
        Paint::red("Invalid path {}",),
        link.display(),
        Paint::red("Skipping...")
    )))?;
    let link_parent = fs::canonicalize(link_parent).map_err(|_| {
        Error::LinkError(format!(
            "{} '{}' {}",
            Paint::red("Cannot create link"),
            link.display(),
            Paint::red("because the parent directory does not exist. Skipping...")
        ))
    })?;

    let link_filename = link
        .file_name()
        .ok_or(Error::LinkError(format!(
            "{} '{}'. {}",
            Paint::red("Invalid path"),
            link.display(),
            Paint::red("Skipping...")
        )))?
        .to_owned();

    Ok((origin, link, link_parent, link_filename))
}

/// Rename a file to `<filename>-backup-<date>`.
///
/// # Errors
/// + [Error::LinkError] if the renaming fails for some reason.
fn backup(parent_dir: &PathBuf, file_name: OsString) -> Result<()> {
    let path = parent_dir.join(&file_name);
    let mut backup_file = file_name;
    let date = chrono::Local::now()
        .format("-backup-%Y-%m-%d-%H-%M-%S")
        .to_string();
    backup_file.push(date);
    let backup = parent_dir.join(backup_file);
    print!(
        "{} '{}'...",
        Paint::yellow("Backing up to"),
        backup.display()
    );
    match fs::rename(&path, backup) {
        Ok(_) => {
            println!("{}", Paint::green("done."));
            Ok(())
        }
        Err(e) => Err(Error::LinkError(format!(
            "{} {}",
            Paint::red("Backup failed."),
            Paint::yellow(e)
        ))),
    }
}

#[derive(Deserialize, Debug)]
struct SymlinkList {
    links: Vec<Link>,
}

#[derive(Deserialize, Debug)]
struct Link {
    path: String,
    origin: String,
}

#[derive(Error, Debug)]
enum Error {
    #[error("The dotfiles directory ({0}) does not exist.")]
    MissingDotfilesDir(PathBuf),
    #[error("The symlink list file ({0}) does not exist.")]
    MissingSymlinkListFile(PathBuf),
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
