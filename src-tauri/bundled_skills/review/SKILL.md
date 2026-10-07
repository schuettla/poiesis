---
name: review
description: Have a reviewer go over the changes I made, and report findings without changing anything.
when_to_use: Only when the user runs /review.
argument-hint: focus (optional)
disable-model-invocation: true
---

Have a reviewer go over the changes I made in this conversation. Focus: $ARGUMENTS

1. Call `changes` to get the files I changed and their patches. If nothing changed, say so and stop.
2. Call `delegate` with a reviewer. Give it the patches from `changes` and the focus above, and tell it to read the surrounding code before judging a line.
3. Ask for findings ranked by how much they matter: bugs first, then risks, then style. Each one names the file, the line, what is wrong, and why it matters.
4. Report the findings to the user as you received them. Do not fix anything, and do not edit any file. This is a review, not a repair.
