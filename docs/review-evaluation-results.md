# Review evaluation results

These results come from the synthetic review evaluations described in
[Codex PR-review evaluations](codex-review-evaluation.md), run between 2026-10-09 and 2026-10-11 with
Claude Code (`claude-haiku-5-5` and `claude-sonnet-5-5` at medium effort) and Codex
(`gpt-6.1-sol` at high effort). They are exploratory samples: two cases per stage, two repeats
per case, and a single blind grader per answer. They decided the review brief defaults and the
bundled skill wording on this branch; they do not establish a general percentage saving. The sealed
holdout B then showed a quality regression for guided Claude Code agents on one case; the fix for
it was measured on the fresh sealed holdout C.

Raw run records, answers, grades and private oracles stay outside Git. Only the aggregates below
are published.

## Method

| Element | Definition |
|---|---|
| Arms | `baseline`: the native review with no RepoScout. `guided`: the same task plus "Start the review with the reposcout skill." It measures what RepoScout is worth once used; it is not the default user experience. `silent`: the skill installed and never mentioned, which measures unprompted adoption. |
| Stages | `complex-development` (`api-migration`: 48 files, about 15k diff tokens with a defect; `module-split`: 116 files, about 101k diff tokens), `complex-holdout` (`config-defaults` with a defect; `behavior-preserving-refactor` without one), and the sealed `complex-holdout-b` (`change-c`, `change-d`) and `complex-holdout-c` (`change-e` with a defect; `change-f` without one). |
| Grading | One `claude-sonnet-5-5` grader at high effort per model and case scores every answer 0–3 against the private oracle. Answers are shuffled and stripped of tool and skill mentions; the grader sees each answer's recorded executions. An answer finding that contradicts an oracle goes to an independent read-only adjudicator; a confirmed oracle gap is reported as a second, amended score next to the strict one. |
| Tokens | `input` is the total model input, including cache reads and cache writes. Cost-weighted input is uncached input + 0.1 × cache read + 1.25 × cache write + 5 × output. All token figures are sums over the cell's runs. |

Scores are sums over the cell's answers, out of 3 per answer.

## Unprompted adoption (Claude Code)

`scripts/agent-eval/adoption_eval.py`, silent arm, at most six turns, the three
`large-review/development` cases. Adoption means the agent loads the skill or runs `reposcout` on
its own. Phrasings are split into `dev`, `tuning` and `held_out` sets; a held-out set that decided
once became a tuning set.

| Phrasings | Description | Haiku | Sonnet |
|---|---|---|---|
| dev | previous | 6/12 | 12/12 |
| dev | A | 12/12 | 12/12 |
| dev | B | 11/12 | 12/12 |
| dev | C | 12/12 | 12/12 |
| first held-out (h1–h4) | previous | 0/12 | 12/12 |
| first held-out (h1–h4) | A | 7/12 | 12/12 |
| tuning (h1–h4) | D | 11/11 | – |
| second held-out (h5–h8) | previous | 2/12 | – |
| second held-out (h5–h8) | A | 3/12 | – |
| second held-out (h5–h8) | D | 11/11 | 10/10 |

Sonnet adopts with every description. Haiku without adoption opens with `git diff`. Description D,
which names the task categories (review, merge or ship safety, what changed, defects, regressions
and risks) and the `git diff` and `git log` alternatives, is the canonical skill description.
Earlier Codex runs with a neutral availability sentence showed no unprompted adoption on the small
`large-*` cases; Codex adoption was not measured again.

## Complex development stage

Skill v4 and brief v2 (before the brief rewrite rules), first look:

| Model | Arm | Score | Input | Cache read | Cache write | Output | Cost-weighted |
|---|---|---|---|---|---|---|---|
| Haiku | baseline | 11/12 | 5505k | 5242k | 262k | 66.6k | 1185k |
| Haiku | guided | 11/12 | 1739k | 1553k | 186k | 40.1k | 588k (−50%) |
| Sonnet | baseline | 7/12 | 684k | 570k | 114k | 7.5k | 237k |
| Sonnet | guided | 9/12 | 548k | 450k | 98k | 8.6k | 211k (−11%) |

The brief then showed no hunks on these comparisons, so guided agents still fetched 50–84 KB of
diff afterwards.

## Complex holdout stage

Spent after two looks; it no longer decides anything. The second look, with brief v3:

| Model | Arm | Score | Input | Cache read | Cache write | Output | Cost-weighted |
|---|---|---|---|---|---|---|---|
| Haiku | baseline | 8/12 | 5533k | 5227k | 306k | 82.2k | 1316k |
| Haiku | guided | 8/12 | 3492k | 3270k | 222k | 60.3k | 906k (−31%) |
| Sonnet | baseline | 5/12 | 1225k | 1095k | 131k | 11.6k | 331k |
| Sonnet | guided | 7/12 | 1711k | 1574k | 136k | 16.6k | 411k (+24%) |

The first look, with brief v2, gave the same scores at −23% (Haiku) and about +21% (Sonnet)
cost-weighted input against these baselines.

Brief v3 adds rewrite rules, compact hunks and moved-code summaries. On the 116-file refactor it
filled its former 8k-token budget (26.7 KB) and stayed in context for every later turn; Sonnet's
roughly 17 calls on that case made the larger brief cost more than the reads it saved. Guided
answers were correct but thinner: they cited fewer of the oracle's evidence obligations and rarely
ran tests or witnesses.

## Brief and skill variants on the development stage

Guided arm only, brief v3, four answers per cell, graded blind in one batch. The baselines are the
development-stage baselines, re-graded in an earlier batch (Haiku 11/12, Sonnet 6/12).

| Variant | Change |
|---|---|
| v3 | Brief v3 at its 8k-token default |
| b4k | v3 with a 4,096-token brief |
| ev | v3 with an evidence sentence in the skill: rule, head line and affected caller per finding |
| both | b4k and ev |

| Model | Arm | Score | Input | Cache read | Cache write | Output | Cost-weighted |
|---|---|---|---|---|---|---|---|
| Haiku | baseline | 11/12 | 5505k | 5242k | 262k | 66.6k | 1185k |
| Haiku | v3 | 10/12 | 1553k | 1391k | 162k | 37.1k | 527k |
| Haiku | b4k | 8/12 | 955k | 832k | 123k | 34.5k | 409k |
| Haiku | ev | 10/12 | 1080k | 952k | 128k | 30.9k | 410k |
| Haiku | both | 12/12 | 1977k | 1823k | 154k | 45.2k | 601k |
| Sonnet | baseline | 6/12 | 684k | 570k | 114k | 7.5k | 237k |
| Sonnet | v3 | 10/12 | 426k | 335k | 91k | 7.2k | 183k |
| Sonnet | b4k | 12/12 | 416k | 344k | 72k | 6.9k | 159k |
| Sonnet | ev | 10/12 | 499k | 411k | 88k | 9.4k | 198k |
| Sonnet | both | 11/12 | 550k | 474k | 77k | 7.9k | 183k |

`both` scored highest overall (23/24) at about half of Haiku's and about three quarters of
Sonnet's baseline cost-weighted input. Haiku with `both` ran tests and witnesses again, which the
shorter brief alone did not restore. The release default is therefore a 4,096-token brief and
the evidence sentence in the skill's review step; the skill description is unchanged.

## Sealed holdout B

First and only look at `complex-holdout-b`, with the `both` variant (4,096-token brief and the
evidence sentence), baseline and guided, two repeats per case. `change-c` introduces a documented
defect. The oracle of `change-d` expects no issues, but every baseline answer reported one more
defect, and an independent read-only adjudication confirmed it with medium confidence. The new
work types require a per-visit meter reading that can legitimately decrease, while the unchanged
asset service rejects a lower reading after the job has already been stored as completed. The
event is then never published, and a retry fails. Every change-d answer was therefore graded
twice:

- `strict` grades against the oracle as written;
- `amended` adds this defect to it, with the same rubric.

Answers that conclude no issues score 0 under the amended oracle.

| Model | Arm | Strict | Amended | Input | Cache read | Cache write | Output | Cost-weighted | Executions |
|---|---|---|---|---|---|---|---|---|---|
| Haiku | baseline | 6/12 | 12/12 | 4532k | 4276k | 256k | 77.4k | 1135k | 15 |
| Haiku | guided | 7/12 | 4/12 | 1484k | 1331k | 153k | 39.5k | 522k (−54%) | 0 |
| Sonnet | baseline | 4/12 | 9/12 | 1132k | 1027k | 104k | 15.8k | 312k | 2 |
| Sonnet | guided | 9/12 | 5/12 | 1128k | 1023k | 105k | 12.3k | 295k (−5%) | 3 |
| Codex | baseline | 4/12 | 8/12 | 2339k | 2065k | – | 28.6k | 624k | 8 |
| Codex | guided | 5/12 | 9/12 | 2930k | 2625k | – | 38.3k | 759k (+22%) | 14 |

`Executions` counts the recorded shell commands of the cell's four answers that invoke Node or
Python; a few are lookups such as `which node`. Codex reports no cache-write field, so its
cost-weighted input treats every uncached token at full price.

How often each arm found the adjudicated change-d defect, in two runs per cell:

| Model | Baseline | Guided |
|---|---|---|
| Haiku | 2 | 0 |
| Sonnet | 2 | 0 |
| Codex | 2 | 2 |

The strict scores repeat the earlier pattern: guided matches or beats the baseline, and Haiku
and Sonnet do so at lower cost. The amended scores show a quality regression for both Claude
models. Guided Claude agents never followed the inputs that the new variants require into the
unchanged shared code that validates and stores them. The baselines did exactly that, with one
search outside the changed files and one read.

The brief listed the new handlers' importers but not where shared code dispatches them, and it
did not mark that gap. Haiku guided ran no check in any run, against 3–5 per baseline run. The
skill's economy wording ("read only what the brief does not answer") and its evidence sentence
favor citations over execution. Codex guided executed as often as its baseline and found the
defect. The `both` variant is therefore not good enough for Claude Code, and the follow-up is
measured on a fresh sealed holdout.

## Fix after holdout B

Two changes address the holdout B regression:

- The brief adds an `unresolved dispatch` line. It names added methods that reuse an existing
  method name without a resolved caller, with up to three receiver call sites, and stays at every
  budget rung. The brief's note now calls its impact static and asks the agent to run the
  repository checks before concluding.
- The skill calls RepoScout output static evidence of what changed, not of how the code behaves.
  For new variants of an existing registry, interface, enum or config, it asks the agent to follow
  each required input or emitted value into the unchanged code that validates, stores or orders it.
  A no-issues verdict needs at least one executed check, and each finding, or else the riskiest
  traced path, gets one minimal witness through the public entry point.

A tuning check on the spent holdout B ran the guided arm only, two runs per case. These answers
were checked against the oracle and the adjudicated change-d defect, not graded blind. The
baselines are the holdout B baselines above.

| Model | Arm | Defects found | Input | Cache read | Cache write | Output | Cost-weighted | Executions |
|---|---|---|---|---|---|---|---|---|
| Haiku | baseline | 4/4 | 4532k | 4276k | 256k | 77.4k | 1135k | 15 |
| Haiku | guided | 4/4 | 3981k | 3729k | 252k | 66.8k | 1022k (−10%) | 14 |
| Sonnet | baseline | 4/4 | 1132k | 1027k | 104k | 15.8k | 312k | 2 |
| Sonnet | guided | 3/4 | 1277k | 1159k | 119k | 14.1k | 335k (+7%) | 2 |

`Defects found` counts the runs that found their case's defect: the documented one in change-c
and the adjudicated one in change-d. Guided Haiku ran checks again and found the change-d defect in
both runs, at about twice its earlier guided cost. One of the two guided Sonnet runs on change-d
traced the new definition fields but not the inputs that completion requires, ran no witness and
concluded no issues. One more skill sentence, asking agents to read each dispatcher site, did not
help: Sonnet found the change-d defect in neither run. It was discarded. Codex was not rerun on
holdout B.

## Sealed holdout C

First and only look at `complex-holdout-c`, with the fix frozen before the first run, baseline and
guided, two repeats per case. `change-e` introduces a documented defect; `change-f` expects no
issues.

| Model | Arm | Score | Input | Cache read | Cache write | Output | Cost-weighted | Executions |
|---|---|---|---|---|---|---|---|---|
| Haiku | baseline | 6/12 | 3633k | 3366k | 267k | 89.5k | 1118k | 8 |
| Haiku | guided | 9/12 | 2573k | 2391k | 181k | 57.4k | 753k (−33%) | 6 |
| Sonnet | baseline | 7/12 | 779k | 690k | 89k | 9.7k | 228k | 2 |
| Sonnet | guided | 10/12 | 1097k | 994k | 103k | 10.6k | 281k (+23%) | 4 |
| Codex | baseline | 4/12 | 1964k | 1703k | – | 20.3k | 533k | 7 |
| Codex | guided | 5/12 | 2179k | 1958k | – | 25.0k | 542k (+2%) | 9 |

Two of guided Sonnet's four executions and one of baseline Haiku's eight were lookups (`which
node`, `node --version`).

Scores per case, out of 6:

| Model | Arm | change-e | change-f |
|---|---|---|---|
| Haiku | baseline | 5 | 1 |
| Haiku | guided | 5 | 4 |
| Sonnet | baseline | 5 | 2 |
| Sonnet | guided | 6 | 4 |
| Codex | baseline | 4 | 0 |
| Codex | guided | 5 | 0 |

Every one of the twelve change-e runs found the defect, so that case did not separate the arms.
The difference comes from change-f. Guided Claude answers concluded no issues with verifiable
evidence and an executed test-suite run in every run. Baseline Claude answers gave weaker evidence;
one Haiku baseline run executed nothing, and another reported an issue. On change-e, neither
Sonnet arm executed anything; the other guided runs executed at least one check each.

Five change-f answers, all four Codex answers and one Haiku baseline answer, reported the same
behavior. The new programme schedule report and visit listing show visits that fell due before a
programme was created or resumed as unopened, although generation never opens them. An
independent read-only adjudication judged it not a defect with medium confidence: the whole
feature is new, the documented rule covers generation only, and the report keeps its documented
promise. A second pass over the four Codex answers found no new rule or evidence. The scores above
are therefore strict. Had the behavior counted as a defect, guided Claude answers would have missed
it in all four runs, against one of four Claude baseline runs that reported it; Codex reported it
in both arms.

Against holdout B, the fix restored executed checks for guided Haiku (6 executions, against 0)
while keeping it a third cheaper than its baseline. Guided Sonnet scored higher at 23% more
cost-weighted input: its baselines needed only 4–5 turns on change-e and 12–16 on change-f, so the
brief added more context than the reads it saved. Guided Codex matched its baseline's cost. Quality held or improved for
all three models, but only Haiku saved tokens.

## Limits and adverse outcomes

- Each cell holds four answers from two cases. One point is within grading noise: re-grading the
  same development answers moved three of eight scores by one point.
- Runs that the host memory reserve aborted were rerun in fresh campaigns with the same seed and
  stay recorded as adverse outcomes. An earlier process-tracking race aborted runs with heavy shell
  verification until it was fixed.
- Development-stage campaigns before 2026-10-09 lacked `mkdir`, `mktemp`, `rm`, `tar`, `tr`,
  `which` and `diff` in the sandbox; agents in every arm lost turns on that. Later stages include
  them.
- Guided savings apply only once an agent uses RepoScout; the adoption table above measures how
  often that happens unprompted.
- Holdout B: the first Codex campaign did not start because the host's temporary-file quota was
  exhausted. It stays recorded, and a fresh campaign with the same seed ran all eight assignments.
- Holdout B: in 4 of 16 Claude Code runs, across both arms, the permission mode denied one compound
  shell command. One guided Haiku run then assumed that every shell command was blocked.
- Graders see only the executions that a command filter keeps. The first Codex grading missed
  commands wrapped in `bash -lc`. The filter was fixed and the Codex answers were graded again
  before unblinding; the Claude Code records were unaffected.
- Holdout B tuning check: the Haiku campaign of the discarded skill sentence failed at preflight
  because the temporary-file quota was exhausted. It was not rerun after the sentence was dropped.
- Holdout C: one guided Haiku run aborted when process-identity protection was unavailable and
  recorded no usage. Its unconfirmed process cleanup left the serial run lease blocked until the
  lease was cleared by hand; a fresh campaign with the same seed then completed the run. Both
  attempts stay recorded.
- Holdout C was sealed against tuning, not against the orchestrating agent. The report that
  generated it named the case with the defect, and grader reports later quoted oracle content.
  The orchestrator also saw the tool calls of the aborted run while diagnosing it. The skill and
  the binary were frozen before the first holdout C run, and nothing from holdout C changed them.
