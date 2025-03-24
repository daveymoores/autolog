# Autolog

A simple tool to generate timesheets from your Git history, built for freelance engineers.

> ⚠️ **Note**: This project is currently in beta. Issues and feedback are welcome.

## Overview

Autolog scans your Git commits to identify worked days, helping you track client projects without manual time logging. Just initialize once per repository, and at the end of the month, generate a timesheet in seconds—collating all worked days across multiple projects. A simple way to turn your commits into a shareable timesheet.

## 🚀 Key Features

- 📌 **Automated Timesheets** – Instantly generate timesheets by pulling commit history from your Git repositories
- 🔗 **Private, Temporary Links** – Generate a **one-time** link (valid for 24 hours) to share timesheets, without storing data online long-term
- 🗂 **Multi-Client & Multi-Repo Support** – Track multiple clients and repositories within a single tool, making it easy to manage complex projects
- 📝 **Multi-Project Support** – Easily track multiple projects within a single tool, making it easy to manage complex projects
 – Track hours across multiple repositories and clients with ease
- 💾 **Local-First Data Storage** – Keep all your data stored **locally** with no sign-up required
- ⚡ **Minimal Setup** – Just initialize your repositories once, then generate monthly timesheets in seconds

## 🔧 Installation

### MacOS (via Homebrew)

```bash
# Install with homebrew (only on MacOS)
brew tap daveymoores/autolog
brew install autolog
```

## Basic Usage

```bash
# Initialize autolog for a repository
autolog init

# Generate a timesheet for January
autolog make -m1

# Modify an entry
autolog edit -d22 -m11 -y2020 -h6
```

## Sample Timesheet

To see a sample timesheet, visit [the sample timesheet page](https://autolog.dev/timesheet-demo).

## Documentation

For full documentation, visit the [documentation page](https://autolog.dev/documentation).

## Data Protection & Privacy

Autolog is designed with a privacy-first approach, minimizing data storage and transmission, and is provided as-is for your use:

- **Local Data Storage**: Your primary data is stored locally within a SQLite database on your computer. This means your Git history and time records never leave your machine during normal operation.
- **Temporary Cloud Storage**: When generating shareable timesheets, a subset of your data is temporarily stored on external servers for a strict 24-hour period. This allows you to share timesheets with clients or teammates via a unique link.
- **Limited Data Sharing**: Only the specific timesheet data you choose to share is transmitted to the server—never your entire database.
- **Automatic Deletion**: All data stored on external servers is automatically and permanently deleted after 24 hours, regardless of whether it was accessed or not.
- **No User Accounts**: Autolog doesn't require user registration, collecting only the minimum data necessary for the timesheet sharing functionality.

## License

Autolog is released under the [MIT License](./LICENSE).
