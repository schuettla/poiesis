# Dual-Process Agent Runtime (DPAR)

**Document type:** Concept and technical design brief  
**Status:** Research prototype proposal  
**Purpose:** Give an AI coding/research agent enough context to design or implement an experimental agent harness that combines fast decision models with slower generative LLMs.

---

## 1. Executive summary

Build an **adaptive cognitive runtime** that combines two complementary model capabilities:

- **Fast decision models:** low-latency, bounded-output models for classification, ranking, routing, candidate selection, triage, and structured judgments.
- **Slow deliberation models:** generative LLMs for planning, interpreting ambiguous goals, reasoning across steps, exploring alternatives, explaining decisions, and revising conclusions.

The innovation is not simply calling two models in sequence. It is the **control system around them**: a runtime that chooses which capability to use, detects when further reasoning or external evidence is worthwhile, allocates a bounded computation budget, tracks outcomes, and learns from failures.

Working principle:

> Think fast by default. Think deeply when the situation warrants it. Verify what matters. Learn from the outcome.

The concept is inspired by Daniel Kahneman's *Thinking, Fast and Slow* and dual-process theories, but it should be implemented as an engineering abstraction—not as a literal simulation of the human brain. Human System 1 and System 2 are not two perfectly separate biological modules, and neither a fast decision model nor a slower LLM is inherently correct.

### Core research question

Can an agent that adaptively switches between fast decisions, deliberate reasoning, and external verification achieve a better **reliability–latency–cost trade-off** than a single LLM or a static two-model pipeline?

### Proposed working name

**Dual-Process Agent Runtime (DPAR)**. Treat the name as provisional.

---

## 2. Design goals

1. Combine fast, structured decisions with flexible, generative reasoning.
2. Let the LLM request a decision from a fast model at any appropriate point in its workflow.
3. Let the controller escalate a fast-model decision to a slow model or evidence-gathering step.
4. Use external evidence, deterministic checks, and tests wherever possible instead of relying on model agreement alone.
5. Allocate compute based on uncertainty, impact, novelty, disagreement, expected value of additional work, and remaining budget.
6. Make every meaningful decision observable and replayable.
7. Evaluate improvements through controlled experiments and ablations.
8. Keep execution permissions and safety boundaries in deterministic runtime code, not in model instructions.

## 3. Non-goals and assumptions

- Do not assume this architecture will automatically outperform a single LLM. That is the main hypothesis to test.
- Do not treat a fast model as a truth oracle or an LLM as an inherently more reliable judge.
- Do not assume that model agreement is independent confirmation. Models may share biases, training data, and failure patterns.
- Do not start by building a swarm of autonomous agents. Begin with one LLM, one decision model, a controller, an evidence interface, and deterministic policies.
- Do not require a natural-language explanation from a decision model designed for bounded structured choices.
- Do not let a model authorize its own permissions, raise its own budgets, or bypass execution policy.
- Do not equate a confident output with a calibrated probability. Validate calibration empirically.

---

## 4. Conceptual model

The human-brain analogy motivates a functional division of labor:

| Human-inspired framing | Runtime analogue | Example operations |
|---|---|---|
| Fast, intuitive judgment (System 1 analogue) | Fast decision plane | Classify, rank, select, route, triage, detect anomalies |
| Deliberate, effortful reasoning (System 2 analogue) | Slow deliberation plane | Plan, reason across steps, generate hypotheses, examine alternatives, revise |
| Metacognition | Metacognitive controller | Decide whether to escalate, verify, continue, stop, or spend more compute |
| Perception and evidence gathering | Evidence and verification plane | Retrieve sources, run code, inspect tool output, test factual claims |
| Learning from experience | Outcome memory and policy improvement | Record outcomes, calibrate confidence, tune thresholds, improve decision policies |

These mappings are engineering abstractions. They are not claims that AI models possess human-like mental processes.

---

## 5. High-level architecture

```mermaid
flowchart TD
    A[Task / user input] --> B[State builder]
    B --> C[Metacognitive controller]
    C -->|Fast path| D[Fast decision model]
    C -->|Deliberation| E[Generative LLM]
    C -->|Need evidence| F[Evidence and verification plane]
    D --> C
    E -->|decide() tool call| D
    E --> F
    F --> C
    C --> G[Deterministic action gate]
    G --> H[Execute / abstain / human approval]
    H --> I[Outcome recorder]
    I --> J[Evaluation and policy improvement]
    J -. offline updates .-> C
```

The architecture is a **dynamic graph**, not a fixed pipeline. The controller can send a task directly to a decision model, invoke an LLM, ask for external evidence, request independent review, or stop.

### 5.1 State builder

Create a compact, typed representation of the current task rather than repeatedly sending an arbitrary transcript dump to every model.

Suggested state fields:

- Task ID and task type
- User goal and success criteria
- Current agent state and completed steps
- Constraints and permissions
- Candidate actions or decision options
- Relevant observations and tool results
- Hypotheses or claims currently under consideration
- Evidence and provenance references
- Outstanding uncertainty and known conflicts
- Estimated impact of an incorrect decision
- Novelty/out-of-distribution indicators, if available
- Remaining time, token, tool-call, and cost budgets

Keep large source documents and conversation history outside the state object and refer to them by stable IDs when possible.

### 5.2 Fast decision plane

Use bounded decision models for tasks with a defined decision space, such as:

- Select the next tool or action from an allowed set.
- Rank candidate documents, plans, search results, or hypotheses.
- Classify an incoming task or claim.
- Prioritize which items deserve verification.
- Assess whether evidence appears to support a specific claim.
- Detect anomalies or potential constraint violations.
- Recommend `continue`, `escalate`, `abstain`, or `request_evidence`.

Design every decision contract with explicit allowed choices and an `unknown`, `abstain`, or `escalate` option when appropriate. Never force the model to choose a substantive answer when the evidence is insufficient.

A fast model may return scores or candidate probabilities, but those numbers must be assessed for calibration on the target task before they are treated as confidence.

### 5.3 Slow deliberation plane

Use the generative LLM when the task benefits from flexible interpretation or multi-step reasoning:

- Interpret ambiguous goals and constraints.
- Break a task into a plan.
- Generate and compare hypotheses or strategies.
- Decide what evidence to collect.
- Use tools and interpret their results.
- Reconsider earlier assumptions when new evidence arrives.
- Explain a conclusion or communicate residual uncertainty.
- Ask the user a clarifying question when genuinely necessary.

Expose a structured `decide()` tool to the LLM so it can consult the fast model during its own workflow. The controller must also retain the ability to invoke the LLM independently.

### 5.4 Evidence and verification plane

Keep evidence gathering separate from model judgment wherever possible. This plane may:

- Search and retrieve authoritative sources.
- Check publication dates and source provenance.
- Retrieve relevant passages and associate them with claims.
- Validate citations and URLs.
- Execute reproducible calculations or code.
- Run tests, schema validation, or static checks.
- Compare a proposed action with observed tool results.

Use deterministic verification where it is feasible. Evidence quality is not established merely because a model describes a source as reliable.

### 5.5 Metacognitive controller

The controller chooses the next operation, manages resource budgets, and defines stopping behavior. It considers:

- **Uncertainty:** empirically calibrated uncertainty for the task and model.
- **Impact:** likely consequence and severity of a bad decision.
- **Novelty:** how different the current input is from evaluated cases.
- **Disagreement:** conflicts between decision models, the LLM, evidence, or repeated judgments.
- **Evidence quality:** relevance, authority, freshness, and consistency of the available evidence.
- **Expected value of additional work:** whether another model call, retrieval, test, or review is likely to improve the result enough to justify its cost.
- **Remaining budget:** latency, tokens, tool calls, and monetary cost.
- **Progress:** whether the latest operation produced new evidence or materially changed the state.

A simple starting heuristic is:

`escalation_score = alpha * uncertainty + beta * impact + gamma * novelty + delta * disagreement`

This is a tunable engineering heuristic, not a psychological law. Define signal scales and weights explicitly, tune them on evaluation data, and use hard risk gates for consequential actions rather than letting a weighted average override those gates.

The controller should ask for more work only when that work is expected to help. More tokens, more models, or more agents are not inherently better.

### 5.6 Deterministic action gate

Before executing an action, check relevant policy and execution constraints in ordinary runtime code:

- Is this action permitted?
- Does it require human approval?
- Is it reversible?
- Is the action within its resource budget?
- Are required inputs validated?
- Have necessary evidence or preconditions been satisfied?
- Is the system allowed to execute this tool with the current credentials and scope?

A model's confident recommendation must never override a denied permission or a mandatory approval step.

### 5.7 Outcome memory and learning

Record the original decision state, model and policy versions, model outputs, evidence, selected action, action result, and eventual outcome. Use the logs for:

- Confidence calibration.
- Tuning escalation and stop thresholds.
- Identifying model/task combinations with poor performance.
- Creating labeled examples for specialist decision models.
- Regression testing new prompts or policies.
- Learning which extra operations provide useful value.

A failure should produce a testable lesson or labeled case, not merely a memory entry that says “be more careful.” Any learned changes should be evaluated on held-out tasks before deployment.

---

## 6. Runtime modes

### Mode 1 — Reflex / fast path

**Use when:** The task is familiar, low-stakes, reversible, and well represented in the fast model's evaluation data.

**Example:** Route a support ticket, rank retrieval candidates, classify a known request type, or select a routine tool.

**Behavior:** The decision model recommends an option. The controller checks policy and execution constraints. If valid, the action can proceed without invoking the LLM.

### Mode 2 — Assisted deliberation

**Use when:** The task requires multiple steps but has moderate uncertainty and manageable stakes.

**Behavior:** The LLM plans and interprets the task, then calls the decision model at branch points to rank tools or candidate actions. It uses the result as input, not as unquestionable authority.

### Mode 3 — Reflective verification

**Use when:** The task is novel, important, evidence-sensitive, or characterized by conflicting signals.

**Behavior:** Gather evidence, run independent checks, inspect competing hypotheses, and involve the slow reviewer selectively. Disagreement is a reason to investigate, not a contest to be won.

### Mode 4 — Controlled execution

**Use when:** An action is consequential, irreversible, safety-sensitive, or crosses a permission boundary.

**Behavior:** Enforce deterministic policies, sandboxing, and authorization. If uncertainty cannot be resolved sufficiently, abstain or request human approval. More model reasoning cannot grant extra permission.

---

## 7. Mutual consultation patterns

### LLM → fast decision model: “Help me choose.”

The LLM supplies a well-defined state, decision question, and list of allowed options. The decision model ranks or selects candidates. The LLM interprets the result and continues its workflow.

### Fast decision model → LLM/controller: “This needs deeper reasoning.”

The decision model can flag ambiguity, conflict, low reliability, or an unfamiliar case. The controller escalates to an LLM, additional evidence, or human review as appropriate.

### Both → evidence system: “The current state is insufficient.”

Instead of asking the models to repeat the same judgment, the runtime retrieves new evidence, performs a calculation, runs a test, or obtains another observation that can actually change the decision.

### Avoid circular escalation

A repeated review must produce new evidence, a materially changed state, or a justified new approach. Set hard limits for depth, total calls, and cost. Stop when no useful progress is being made, then return a qualified answer or abstain.

---

## 8. Structured decision interface

Expose a narrow, typed contract. Example request:

```json
{
  "task": "select_next_action",
  "state": {
    "goal": "verify an answer",
    "unresolved_claims": 3,
    "evidence_available": true
  },
  "question": "What should happen next?",
  "options": [
    "accept",
    "retrieve_more_evidence",
    "request_slow_review",
    "abstain"
  ]
}
```

Example response (illustrative only; these are not real model outputs):

```json
{
  "decision": "retrieve_more_evidence",
  "scores": {
    "accept": 0.08,
    "retrieve_more_evidence": 0.71,
    "request_slow_review": 0.17,
    "abstain": 0.04
  },
  "escalation_required": false,
  "reason_codes": [
    "insufficient_evidence"
  ],
  "model_version": "decision-model-version",
  "decision_contract_version": "1"
}
```

Implementation notes:

- Validate requests and responses against a schema.
- Reject choices outside the allowed option set.
- Treat scores as uncalibrated until tested.
- Define whether scores must sum to one and whether they represent probabilities or relative scores. Do not mix those meanings.
- Keep machine-readable `reason_codes` separate from any optional explanation text.
- Keep the chosen action separate from execution; the controller and action gate own execution.
- Version decision contracts so traces remain interpretable after changes.
- Store the model ID, prompt/policy version, latency, and usage/cost metadata when available.

---

## 9. Flagship use case: fact-checking LLM output

This is a strong first experiment because it separates fast triage, slow interpretation, and external evidence into testable roles.

### Workflow

1. **Generate and decompose.** The LLM creates a response and identifies individual factual claims. Give every claim a stable ID and capture any source reference or citation.
2. **Fast triage.** The decision model categorizes claims and prioritizes verification by uncertainty, consequence, and time sensitivity.
3. **Retrieve evidence.** Search authoritative sources, preserve publication dates, retrieve relevant passages, and validate source references. Use deterministic calculations or executable checks for suitable numeric claims.
4. **Fast evidence assessment.** Given a claim and retrieved passages, the decision model selects among `supported`, `refuted`, `insufficient_evidence`, and `conflicting_evidence`.
5. **Selective slow review.** The LLM examines disputed or important claims, checks whether the source actually entails the claim, seeks more evidence when appropriate, and revises the draft.
6. **Final gate.** Check high-impact claims, citation relevance, internal consistency, and whether corrections introduced unsupported statements. Mark unresolved claims explicitly.
7. **Outcome recording.** Store the verdict, evidence, any correction, and the ground-truth label when available.

### Why not simply ask another LLM, “Are you sure?”

Fast triage can reduce the amount of slow review required. The slow model can reason about subtle contexts and source meaning. The evidence system supplies an external reference that can challenge either model. Prompted self-critique alone is not reliable proof of truth; external feedback tends to be more useful when it is itself trustworthy.

**Important limitation:** A decision model cannot establish truth merely by returning a confident verdict. Evaluate it on evidence-grounded tasks and use it as one component of verification, not as an oracle.

### Example claim record

```json
{
  "claim_id": "claim-004",
  "claim_text": "Example factual assertion from the answer",
  "claim_type": "time_sensitive_fact",
  "source_refs": ["source-12"],
  "priority": "high",
  "fast_verdict": "insufficient_evidence",
  "slow_review_required": true,
  "final_verdict": "unresolved",
  "notes": "The available source does not establish the full assertion."
}
```

---

## 10. Social-science principles translated into runtime design

### 10.1 Protect independent judgment

A fast verdict can anchor a slow reviewer. On important cases, run a blind review where the LLM inspects the evidence before seeing the fast model's verdict, then compare the results. Measure how order and exposure to prior judgments affect accuracy.

### 10.2 Separate confidence from correctness

Measure calibration by task and, where feasible, by relevant input subgroup. If a system assigns 0.9 probability to an outcome on many comparable cases, that outcome should occur approximately 90% of the time for the estimate to be well calibrated. Evaluate calibration rather than assuming it from output formatting.

### 10.3 Allocate effort according to consequences

A poor ranking may be cheaply corrected; a destructive operation may not be. Use risk-sensitive thresholds that consider the likelihood and severity of error. Confidence alone must not authorize consequential action.

### 10.4 Learn from outcomes, not rationalizations

Compare predicted outcomes with actual outcomes: tool success/failure, test results, rejected arguments, broken citations, user corrections, or independently labeled answers. Retrospective explanations are not a substitute for observed outcomes.

### 10.5 Treat disagreement as information

When the models disagree, inspect the source of disagreement and seek new evidence. Do not automatically trust the larger model, assume the fast model is more objective, or simply take a majority vote.

### 10.6 Preserve diversity of evidence

Model outputs are often not independent. When correctness matters, prioritize independent sources, deterministic validators, executable tests, and evidence with traceable provenance.

---

## 11. Experimental harness and observability

Make experimentation a first-class feature.

### Record each run

- Run ID, task ID, timestamp, benchmark/split ID
- Input state and relevant constraints
- Agent, decision-model, controller, prompt, and policy versions
- Every decision request and response
- Allowed options and scores/probabilities
- Confidence calibration version, if applicable
- Escalation and stopping reasons
- Tool calls, retrieved evidence, source metadata, and verification results
- Action proposed, action actually executed, and outcome
- Human intervention and final disposition
- Latency by operation and total latency
- Token usage, tool-call count, and monetary cost where available
- Budget remaining at each decision point

Avoid logging secrets or unnecessary personal data. Make stored traces inspectable and replayable, with access controls appropriate to the workload.

### Suggested metrics

**Quality and reliability**
- Task success rate
- Claim-level support precision and recall
- False acceptance and false rejection rates
- Tool-selection accuracy and tool misuse rate
- Calibration error and Brier score where probability semantics are valid
- Escalation precision/recall
- Failure rate on novel or out-of-distribution tasks
- Safety/policy violations

**Efficiency**
- Median and 95th-percentile latency
- Cost per task and per successful task
- Token and tool-call counts
- Slow-review rate
- Unnecessary review rate
- Number of cycles before stop

**Controller quality**
- Additional work's marginal quality gain
- Fraction of escalations that correct an error
- Fraction of non-escalated cases containing missed errors
- Stop decisions that occurred too early or too late

---

## 12. Hypotheses and experiments

Use held-out tasks and controlled ablations. Keep model versions, prompts, evaluation sets, and budgets recorded so comparisons can be reproduced.

| Hypothesis | Experimental comparison | Primary outcomes |
|---|---|---|
| H1: Fast decisions improve efficiency. | Single-LLM baseline vs. fast-first routing | Cost, latency, task success |
| H2: Selective slow review catches more errors. | Fixed verification vs. uncertainty/disagreement-triggered review | Missed errors, detection rate, review cost |
| H3: External evidence improves verification more than repeated self-review alone. | LLM self-critique vs. evidence-backed dual-process verification | Claim accuracy, false acceptance, citation quality |
| H4: Independent review reduces anchoring. | Blind review vs. review after seeing fast verdict | Accuracy, consistency, reviewer sensitivity to verdict order |
| H5: Adaptive compute improves the quality–cost trade-off. | Fixed reasoning budget vs. adaptive budget | Quality at matched cost/latency |
| H6: Explicit abstention improves high-risk behavior. | Forced choice vs. abstention/escalation option | High-risk false acceptance, useful completion rate |

### Essential ablation matrix

At minimum, compare:

1. LLM alone.
2. Fast decision model alone (for tasks it can meaningfully complete).
3. LLM plus decision model with static routing.
4. LLM plus decision model with adaptive routing.
5. Adaptive dual-process system with external verification.
6. Adaptive dual-process system with independent/blind review where applicable.

Report both average performance and failure slices. A quality improvement that costs ten times more may not justify deployment; a latency improvement that quietly increases critical errors may be unacceptable.

### Initial success criterion

The first milestone is not simply “the dual system beats the LLM.” It is:

> The dual system maintains or improves task quality while reducing cost or latency, and its escalation policy reliably identifies cases where additional work is worthwhile.

---

## 13. Minimum viable prototype

Start with the smallest implementation that tests the main hypothesis.

### MVP components

- One generative LLM adapter.
- One fast decision-model adapter.
- One typed state representation.
- One `decide()` interface callable from the LLM.
- A rule-based metacognitive controller.
- A deterministic action gate with tool and budget limits.
- A basic evidence adapter for retrieval or another verifiable external source.
- Structured trace/event logging.
- A replayable evaluation harness.
- A benchmark containing tool-selection and factual-verification tasks.

### MVP controller rules

Start with transparent rules rather than training a learned policy immediately. Example escalation triggers:

- Low calibrated confidence for the relevant task.
- Conflict between model verdict and evidence.
- Missing evidence for a claim that requires it.
- Input flagged as novel or outside the evaluated distribution.
- High-impact or irreversible action.
- Repeated tool failure or lack of progress.
- A hard permission/policy rule requiring approval.

Add explicit maximums for deliberation depth, tool calls, model calls, latency, and cost. Hard risk gates take precedence over weighted scores.

### Suggested implementation milestones

1. **Contracts and adapters:** define state and decision schemas; implement model adapters and validation.
2. **Controller and limits:** implement rules for fast path, escalation, stopping, and budgets.
3. **Traceability:** log all decisions and support deterministic replay as far as external dependencies allow.
4. **First benchmark:** evaluate tool selection and evidence-backed claim verification.
5. **Ablations:** compare model-only, static dual, and adaptive dual configurations.
6. **Calibration and tuning:** calibrate scores and tune thresholds on development data, then freeze policy and evaluate on held-out cases.
7. **Expansion:** specialize decision functions only when logs show stable, recurring patterns worth specializing.

---

## 14. Future research directions

### Adaptive cognitive budgets

Learn when a quick decision is sufficient, when the LLM should reason longer, and when retrieval, tests, or human review provide the best expected value. This turns routing into a learned control policy rather than a fixed heuristic.

### Specialized decision functions

Instead of one generic decision model, develop specialized policies for tool selection, evidence quality, anomaly detection, prioritization, stopping, or risk triage. The controller chooses which to invoke based on the current state.

### Outcome-driven improvement

Use labeled traces to identify recurring failure classes, generate targeted evaluation cases, and train or tune specialist policies. Promote changes only after held-out evaluation and regression tests.

### More rigorous metacognition

Estimate the marginal expected value of another operation: probability of correcting a relevant error multiplied by the value of that correction, minus expected latency and cost. Validate any such estimate against real outcomes rather than treating it as directly observable.

---

## 15. Failure modes and mitigations

| Failure mode | Why it matters | Mitigation |
|---|---|---|
| Confident fast-model error | A high score can be wrong, especially on unfamiliar inputs | Calibrate per task; test out-of-distribution behavior; allow abstention; escalate uncertain high-impact cases |
| Infinite review loop | Models repeatedly reconsider without progress | Bound calls/depth/cost; require new evidence or a material state change before repeating; stop when marginal value is low |
| False consensus | Models share training data, biases, or assumptions | Use independent evidence, deterministic checks, blind review, and diverse validators |
| Anchoring | A prior verdict changes later judgment | Blind the slow reviewer to the fast verdict on selected tasks; test order effects |
| Rewarding verbosity | Longer reasoning appears thoughtful without improving accuracy | Evaluate outcomes and marginal value, not explanation length |
| Correlated verifier failures | Models repeat the same mistake | Use source-grounded checks and executable validation where possible |
| Policy bypass | A model attempts a forbidden or unauthorized action | Keep permissions and execution in deterministic runtime code |
| Stale evidence | Old facts are treated as current | Record source date and retrieval time; apply freshness rules to time-sensitive claims |
| Trace contamination or privacy leak | Logs may contain secrets or personal data | Minimize stored data, redact sensitive material, and apply access controls and retention rules |
| Overfitting the controller | Thresholds perform well only on tuning set | Tune on development data, freeze policy, and report held-out performance |

---

## 16. Recommended first research project

**Project:** Adaptive fast/slow factual verification and tool routing.

Build the minimal dual-model runtime and evaluate two related tasks:

1. **Tool selection:** Can the fast decision layer reduce LLM calls while maintaining tool-choice quality?
2. **Factual verification:** Can selective review and external evidence catch more unsupported claims at an acceptable cost and latency?

Start with a rule-based controller, then compare against static routing and single-model baselines. Measure whether each escalation was useful, not just whether the final answer was good.

### Deliverables for an implementation agent

1. A concise architecture/design document that maps the proposed components to the existing harness, if any.
2. Typed schemas for agent state, decision requests/responses, evidence records, and event traces.
3. Model adapter interfaces that allow models to be swapped without changing the controller.
4. A controller with explicit escalation, budget, and stopping rules.
5. Deterministic execution and permission boundaries.
6. A reproducible evaluation harness with the essential ablation matrix.
7. Sample traces and tests covering normal flow, escalation, disagreement, abstention, budget exhaustion, invalid model outputs, and denied actions.
8. A short report showing quality, latency, cost, and common failure slices.

If working inside an existing repository, inspect the current architecture and preserve its conventions before making implementation choices. Do not assume a language, framework, provider, or deployment topology that has not been specified. Prefer a small vertical slice over a large speculative rewrite.

---

## 17. Research foundations and further reading

These references motivate aspects of the design. They do not prove that the proposed DPAR architecture will work; that requires empirical testing.

### Cognitive science and dual-process theory

- Kahneman, Daniel. *Thinking, Fast and Slow*. Farrar, Straus and Giroux, 2011. Book overview: https://us.macmillan.com/books/9780374533557/thinkingfastandslow/
- Evans, Jonathan St. B. T., and Keith E. Stanovich. “Dual-Process Theories of Higher Cognition: Advancing the Debate.” *Perspectives on Psychological Science* (2013). https://doi.org/10.1177/1745691612460685
- Dual-process approaches to decision-making and LLMs: treat this as an evolving research area and verify relevant survey/review papers before relying on specific claims.

### Fast decision models

- Cloudflare, “Introducing Clef: our open-source decision models.” https://blog.cloudflare.com/clef-decision-models/
- TypeSafe AI, “Introducing System One Models & Jev.” https://typesafe.ai/blog/introducing-system-one-models-and-jev

Model capabilities, availability, APIs, licenses, and benchmark claims can change. Verify current details directly before selecting a model for the implementation.

### Agents, tools, and feedback

- Schick et al., “Toolformer: Language Models Can Teach Themselves to Use Tools.” https://arxiv.org/abs/2302.04761
- Shinn et al., “Reflexion: Language Agents with Verbal Reinforcement Learning.” https://arxiv.org/abs/2303.11366
- “When Can LLMs Actually Correct Their Own Mistakes? A Critical Survey of LLM Self-Correction.” https://aclanthology.org/2024.tacl-1.78/

### Evaluator bias and calibration

- “Judging the Judges: A Systematic Study of Position Bias in LLM-as-a-Judge.” https://aclanthology.org/2025.ijcnlp-long.18/
- “Classifier calibration: a survey on how to assess and improve predicted class probabilities.” https://doi.org/10.1007/s10994-023-06336-7

---

## 18. Final recommendation

Build DPAR as an **adaptive cognitive runtime**, not a literal brain simulation and not a multi-agent swarm by default.

The key contribution is the orchestration policy that determines which kind of operation is needed, when an initial judgment should be challenged, and what evidence is sufficient before acting. Fast decision models supply low-latency bounded judgments; LLMs supply flexible deliberation; evidence and deterministic validators anchor important decisions; the controller makes the combination measurable and resource-aware.

The first question to answer empirically is:

> Can an agent that adaptively switches between fast decisions, deliberate reasoning, and external verification achieve better reliability–latency–cost trade-offs than either a single LLM or a static two-model pipeline?

That question is specific enough to implement, test, and potentially expand into a broader agent architecture.
