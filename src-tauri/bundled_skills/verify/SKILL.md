---
name: verify
description: Run the project's check and fix whatever fails.
when_to_use: Only when the user runs /verify.
disable-model-invocation: true
---

Run this project's check and fix what fails. $ARGUMENTS

1. Find the project's check: the `check`, `test` or `lint` task. Use the project card or the manifest; do not guess a command.
2. Run it with `run_task`.
3. If it passes, say so and stop.
4. If it fails, read the first failure, fix its cause (not the symptom), and run the check again.
5. Stop after 3 rounds. Report what passed, what you fixed, and what still fails, with the failing output for anything left.
