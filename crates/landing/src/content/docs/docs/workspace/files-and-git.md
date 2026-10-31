---
title: Files and Git
description: Open project files, edit text, and review Git changes.
---

Files and Git run on the computer that hosts the daemon. On a remote connection, you are looking at that remote project.

## Find a file

Use the file tree or file search. Search skips ignored and generated files.

## Edit

The editor opens UTF-8 text files up to 1 MB.

Drafts stay while the editor panel is open. Save before you reload the app or switch connections.

![The project tree and editor, with a TypeScript file open beside the conversation.](/images/workspace/editor.webp)

If someone else changed the file, the save is rejected. Reload the file and apply your edit to the current version.

## Git

The Git view separates unstaged and staged changes. Stage files, write a message, and commit. Git must be installed on the daemon’s computer.

![Git review with staged and unstaged files, an inline diff, and the commit field.](/images/workspace/git-review.webp)

A folder does not have to be a Git repository to be a workspace. Diffs are shown for text files up to 1 MB.
