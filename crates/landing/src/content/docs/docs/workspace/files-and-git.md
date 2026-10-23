---
title: Files and Git
description: Browse project files, edit text, and review staged and unstaged changes.
---

Workspace operations run on the daemon host. With a remote connection, the file tree, editor, and Git views all refer to that remote project.

## Find a file

Use the workspace file tree or file search to open a project file. Discovery respects workspace `.gitignore` files, excludes common generated directories, and currently searches to a maximum depth of six.

## Edit text

The editor supports UTF-8 text files up to 1 MB. Binary files cannot be opened as editable text.

Drafts and undo history stay in memory while you close and reopen the editor panel. They are not durable across app reloads, so save your work before reloading or switching daemon connections.

## Handle a save conflict

A save includes the content originally read from disk. If an agent or another client has changed the file, the daemon rejects the stale save rather than overwriting those changes.

Keep your draft, reload the current file, review the difference, and apply your intended edit against the latest version. The daemon serializes saves from Amarcode clients and prepares replacements in a temporary file.

## Review Git changes

The Git view distinguishes staged and unstaged changes. A file can have both:

- **Unstaged** compares the working tree with the index.
- **Staged** compares the index with `HEAD`.

Review the diff, stage or unstage individual files, or stage all files. Enter a commit message to commit staged changes. Git must be installed and configured on the daemon host.

:::note
A non-Git folder can still be a workspace. Git actions require a repository, and text diff display is limited to files up to 1 MB.
:::
