# Dotconfig

A tool for symlinking configuration files from a central repository to their respective locations on
the system in a repeatable, configurable way.

## Install
```
cargo install dotconfig
```

## Configuration
By default `dotconfig` will look for the directory `~/.cfg`, which is assumed to contain all of your
dotfiles as well as `symlinks.yml`, which is a listing of all of the desired symlinks you would like
`dotconfig` to make for you.

The format of `symlinks.yml` should be as follows:

```yaml
links:
  - link:
    path: ~/.zshrc
    origin: zshrc
  - link:
    path: ~/.config/alacritty/alacritty.yml
    origin: alacritty-config.yml
# ...
```
In this example, `dotconfig` would create the following two symlinks:
```
~/.zshrc -> ~/.cfg/zshrc
~/.config/alacritty/alacritty.yml -> ~/.cfg/alacritty-config.yml
```

## Usage
```
dotconfig [OPTIONS]
```

## Options
```
-c, --config <CONFIG>    Specify the YAML file that lists your desired symlinks [default: symlinks.yml]
-d, --dir <DIR>          Specify the directory that holds your config files [default: $HOME/.cfg]
-h, --help               Print help information
-V, --version            Print version information
```

## Example usage

In the following example, `dotconfig` will read `~/my-dotfiles/links.yml`, and all links will be
made to the `~/my-dotfiles` directory.

```sh
dotconfig -d ~/my-dotfiles -c links.yml
```
