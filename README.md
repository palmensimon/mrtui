# mrtui

A terminal UI for reviewing GitLab merge requests, built with [Ratatui](https://ratatui.rs).

## Features

- Browse open merge requests for a GitLab project
- View MR description, comments, and diff
- Check out MRs locally via `git worktree`
- Quick search with `/`

## Requirements

- Rust (stable)
- A GitLab personal access token with `read_api` scope

## Install

```sh
cargo install --path .
```

## Configuration

On first run, open the Settings view (`s`) and fill in:

| Field | Description |
|---|---|
| GitLab URL | e.g. `https://gitlab.com` |
| Access Token | Your personal access token |
| Project ID | Numeric GitLab project ID |
| Worktree root | Directory where MR branches are checked out |

Config is saved to `~/.config/mrtui/config.toml`.

## License

MIT
