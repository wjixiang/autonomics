---
name: commit
description: "Stage and commit all changes with a conventional commit message"
aliases:
  - ci
  - save
allowed_tools:
  - bash
  - read
  - glob
user_invocable: true
model_invocable: true
---

Examine the current git diff and staged changes, then create a conventional commit message.

Steps:
1. Run `git status` and `git diff` to understand what has changed
2. Categorize changes (feat, fix, docs, refactor, etc.)
3. Write a concise conventional commit message
4. Stage all changes with `git add -A`
5. Commit with the generated message
