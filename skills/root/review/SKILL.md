---
name: review
description: "Perform a thorough code review on recent changes"
aliases:
  - pr-review
allowed_tools:
  - bash
  - read
  - grep
  - glob
  - activate_skill
  - attempt_complete
  - abort_task
user_invocable: true
model_invocable: true
---

Review the most recent changes in the repository.

Steps:
1. Run `git diff HEAD~1` to see the latest commit's changes
2. For each changed file, read its full content
3. Check for:
   - Logic errors or edge cases
   - Missing error handling
   - Performance issues
   - Code style inconsistencies
4. Summarize findings with file paths and line numbers
