---
name: skillify
description: Turn what we just did into a reusable skill.
when_to_use: Only when the user runs /skillify.
argument-hint: name (optional)
disable-model-invocation: true
---

Look back over this conversation and turn the procedure that worked into a skill. Name: $ARGUMENTS

1. Find the repeatable part: the steps that would do the same job again, leaving out what was specific to this one time.
2. Write them as short imperative instructions a future run can follow without this conversation. Keep real commands and file names, drop the one-off details.
3. Call `propose_skill` with a short kebab-case name (use the name above if one was given), a one-line description, a one-line when_to_use, and the body.
4. Tell the user it is waiting for their yes. The skill does not exist until they accept it.

If nothing in the conversation is worth repeating, say that instead of proposing one.
