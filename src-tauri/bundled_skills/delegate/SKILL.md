---
name: delegate
description: Hand a task to a background agent and keep talking.
when_to_use: Only when the user runs /delegate.
argument-hint: task
disable-model-invocation: true
---

Hand this to a background agent: $ARGUMENTS

1. Call `delegate` with `background: true`. The brief is the task above, written so the agent can do it with no other context: what to do, where to look, and what to bring back.
2. Tell the user in one sentence that it is running, and that they can watch it in the Agents panel.
3. Do not wait for it and do not do the work yourself. Carry on with whatever the user says next.
