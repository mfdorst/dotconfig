use clap::Parser;
use serde::Deserialize;
use std::{
    ffi::{OsStr, OsString},
    fs::{self, read_link, File},
    io::BufReader,
    os::unix,
    path::{Path, PathBuf},
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
    let origin = dotfiles_dir.join(origin);
    let origin = canonicalize_origin(&origin)?;
    let link = expand_link_file(&link)?;
    let link_filename = link_filename(&link)?;
    let link_parent = link_parent(&link)?;

    if !link_parent.exists() {
        println!(
            "{} {} {}",
            Paint::yellow("The directory"),
            link_parent.display(),
            Paint::yellow("does not exist. Creating...")
        );
        fs::create_dir_all(&link_parent)?;
    }
    let link_parent = canonicalize_link_parent(&link_parent, &link_filename)?;

    if link.exists() {
        if let Ok(existing_link_origin) = read_link(&link) {
            if origin == fs::canonicalize(&existing_link_origin)? {
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
                backup(&link_parent, &link_filename)?;
            }
        } else {
            print!(
                "{} '{}' {} ",
                Paint::yellow("The path"),
                link.display(),
                Paint::yellow("already exists.")
            );
            backup(&link_parent, &link_filename)?;
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

/// Returns the path to the symlink with all shell variables expanded.
///
/// # Params
/// + `link` - The path to the link file.
///
/// # Errors
/// + [Error::ShellexpandLookupError] if the path contains a shell variable that does not exist in
/// the environment.
fn expand_link_file<P>(link: &P) -> Result<PathBuf>
where
    P: AsRef<str>,
{
    Ok(shellexpand::full(&link)?.into_owned().into())
}

/// Returns the path to the folder the symlink will go in.
///
/// # Params
/// + `link` - The path to the symlink.
///
/// # Errors
/// + [Error::LinkError] if `link` does not have a valid parent directory.
fn link_parent<P>(link: &P) -> Result<PathBuf>
where
    P: AsRef<Path>,
{
    Ok(link
        .as_ref()
        .parent()
        .ok_or(Error::LinkError(format!(
            "{} '{}' {}",
            Paint::red("Invalid path {}",),
            link.as_ref().display(),
            Paint::red("Skipping...")
        )))?
        .into())
}

/// Returns the symlink's filename.
///
/// # Params
/// + `link` - The path to the symlink.
///
/// # Errors
/// + [Error::LinkError] if `link` is not a valid path.
fn link_filename<P>(link: &P) -> Result<OsString>
where
    P: AsRef<Path>,
{
    Ok(link
        .as_ref()
        .file_name()
        .ok_or(Error::LinkError(format!(
            "{} '{}'. {}",
            Paint::red("Invalid path"),
            link.as_ref().display(),
            Paint::red("Skipping...")
        )))?
        .to_owned()
        .into())
}

/// Returns the symlink's parent directory in canonical, absolute form with all intermediate
/// components normalized and symbolic links resolved. See [`fs::canonicalize`].
///
/// # Params
/// + `link_parent` - The path to the symlink's parent directory.
/// + `link_filename` - The symlink's filename.
///
/// # Errors
/// + [Error::LinkError] if `link_parent` does not exist as a path on the system.
fn canonicalize_link_parent<P, S>(link_parent: &P, link_filename: &S) -> Result<PathBuf>
where
    P: AsRef<Path>,
    S: AsRef<OsStr>,
{
    Ok(fs::canonicalize(link_parent).map_err(|_| {
        Error::LinkError(format!(
            "{} '{}' {}",
            Paint::red("Cannot create link"),
            link_parent.as_ref().join(link_filename.as_ref()).display(),
            Paint::red("because the parent directory does not exist. Skipping...")
        ))
    })?)
}

/// Returns the path to the file that should be linked to in canonical, absolute form with all
/// intermediate components normalized and symbolic links resolved. See [`fs::canonicalize`].
///
/// # Params
/// + `origin` - The path to the file that should be linked to.
///
/// # Errors
/// + [Error::LinkError] if `origin` does not exist as a path on the system.
fn canonicalize_origin<P>(origin: &P) -> Result<PathBuf>
where
    P: AsRef<Path>,
{
    Ok(fs::canonicalize(&origin).map_err(|_| {
        Error::LinkError(format!(
            "{} '{}' {}",
            Paint::red("The path"),
            origin.as_ref().display(),
            Paint::red("does not exist. Skipping...")
        ))
    })?)
}

/// Rename a file to `<filename>-backup-<date>`.
///
/// # Errors
/// + [Error::LinkError] if the renaming fails for some reason.
fn backup<P, S>(parent_dir: &P, file_name: &S) -> Result<()>
where
    P: AsRef<Path>,
    S: AsRef<OsStr>,
{
    let path = parent_dir.as_ref().join(file_name.as_ref());
    let mut backup_file = file_name.as_ref().to_owned();
    let date = chrono::Local::now()
        .format("-backup-%Y-%m-%d-%H-%M-%S")
        .to_string();
    backup_file.push(date);
    let backup = parent_dir.as_ref().join(backup_file);
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
