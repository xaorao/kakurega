# Quality eval: asking a model directly against the confirmed-findings review

## The question
Does the review loop give better findings than asking the same model to review the project in one go?

## Setup
- Project: `fixtures/synthetic-repo`, a small payments service. It has one planted defect (logout leaves the session token valid), two things that look like defects and are not, and one weakness that was not planted as the defect but is real when read on its own (a fixed default signing secret). All are listed in `fixtures/synthetic-repo.truth.json`.
- Model: one local model for both arms (`qwen2.5-coder:7b` in the committed run).
- Plain arm: every file of the project in one prompt; the model's list of findings is taken as it is.
- Kakurega arm: the real review through the running service. A finding counts only when its reproducer was run in the sandbox and confirmed.
- Three runs per arm. Reported per arm: every run, the total, and the worst run.

## Scoring
- Found: a finding in the planted defect's file, within 8 lines of it.
- Arguable: a finding in the file of the listed real-but-unplanted weakness. Counted apart, neither as found nor as false.
- False: every other finding.

## Result of the committed run (`results.json`)
| | planted defect found | false findings (total, worst run) | arguable | seconds per run |
|---|---|---|---|---|
| Plain | 3 of 3 | 13, 5 | 0 | 12 to 19 |
| Kakurega | 3 of 3 | 0, 0 | 3 (one per run, each confirmed by its script) | 145 to 155 |

In each Kakurega run the model proposed 8 findings; the sandbox confirmed 2 and turned down 6 (a script whose result did not depend on the file it cited, a script whose result changed between two runs, a script that ended in an error, a script that concluded the defect was absent).

## On NEAR AI Cloud, the same comparison with a larger model (`results-near.json`)
Rerun with `Qwen/Qwen3.8-27B` on NEAR AI Cloud in both arms: the plain arm asks it directly over NEAR's API with every file in one prompt; the Kakurega arm is the real review on the service's NEAR route (confidential hardware, each answer's signature checked). Same project, same scoring, three runs each. Script: `KAKU_TEST_NEAR_KEY_FILE=FILE scripts/eval-quality.py near:Qwen/Qwen3.8-27B 3`.

| | planted defect found | false findings (total, worst run) | arguable | seconds per run | cost of the three runs |
|---|---|---|---|---|---|
| Plain | 3 of 3 | 6, 2 | 3 | 3 to 4 | about USD 0.0007 |
| Kakurega | 3 of 3 | 1, 1 | 3 | 55 to 65 | about USD 0.013 |

- Every false finding of the plain arm was about `.env`: it was handed the secrets file with the rest and reported its contents as findings. Kakurega never gives that file to the model.
- Kakurega's one false finding (third run) is in `payments/api/handlers.py`: "handle_charge enforces no upper bound on amount". The sandbox did confirm the behaviour (a charge of 10^12 cents is accepted), so it is a true statement about the code; it counts as false because the fixture's truth file does not list it, and the scoring was not changed after the run. Its explanation leans on `docs/vendor-notes.md`, which this fixture marks as untrusted text.
- Every finding the larger model proposed was confirmed by its own script (none turned down), against 6 of 8 turned down with the small local model; the review took about a third of the local model's time.
- Cost at NEAR's published price (0.04 and 1.35 US dollars a million tokens read and written): the service's usage record shows 129,079 tokens read and 5,818 written for the three reviews. A prompt NEAR reads again from its cache is billed less, so this is at most what was charged.
- The limits below apply unchanged: one small project, one planted defect, one model per run.

## What this run changed in the product
The first run of this eval had the review find the planted defect in 0 of 3 runs: the small model read two files, said "no defects" and kept saying so. The review now gives every source file a second look on its own once the model says it is done: a fresh question "what could be wrong in this file", then for each suspicion a fresh request to prove it with a script or say it is not real. The scripts go to the same sandbox check as any finding. See `docs/engineering-decisions/worker-loop.md`.

## Limits
- One small project, one planted defect, one model. This shows that confirming findings removed the false ones here and that the second look recovered the defect here. It is not a measure of review quality in general.
- The three Kakurega runs gave the same result, which suggests the review route runs the model without randomness; they are then not three independent samples.
- Money: the model runs on this computer, so no charge. Tokens are in `results.json` for both arms: about 940 in and 300 out per plain run; for the three reviews together the service's usage record shows 63,579 in and 8,334 out, about 21,000 in and 2,800 out per review. With a price entered for a route, the usage screen turns these into money.
- The review takes about ten times as long as the single prompt.
