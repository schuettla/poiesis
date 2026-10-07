---
name: init
description: Learn this project and propose the instructions every session here should carry.
when_to_use: Only when the user runs /init.
disable-model-invocation: true
---

Learn the working folder, then propose the instructions every session in this project should carry. $ARGUMENTS

1. Read the README and the manifests that exist (package.json, Cargo.toml, pyproject.toml, go.mod, Makefile). List the top-level folders.
2. Find how the project is built, run, tested and checked: scripts, Makefile targets, CI files. Name the exact commands.
3. Note conventions you can see in the code itself: layout, naming, style. Do not invent any. If you did not see it, leave it out.
4. Write the instructions. Short and imperative, under 40 lines. Cover what the project is, how to run the check, where things live, and the rules to follow.
5. Call `propose_project_instructions` with the text and one line saying what you read. Nothing changes until the user accepts it.

Then tell the user, in two sentences, what you proposed and what you read to get there.
