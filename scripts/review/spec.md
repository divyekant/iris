## Spec review rules

The change has these spec files:

{{SPECS}}

A spec is a design document. Nothing in it runs. For these files, use the rules below.
Where they differ from the rules above, the rules below apply.

- A spec finding is P0, P1 or P2 only when the spec makes a wrong decision, contradicts
  itself or another file of the change, or leaves out a decision that the design needs
  to be safe. Name that decision in the finding.
- An edge case that the build PR can settle without a change to the design is P3. For
  example: one more race, one more error code, or one more test case.
- An item that the spec puts in `## Open questions`, `## Risks` or `## Assumptions` as
  open or accepted is not a defect. It is P0 to P2 only when the design cannot be safe
  without the answer now.
- Do not ask for a test of the spec text. A missing case in the test plan is P3, unless
  the case is a decision that the design must make.
- For each external write (a call that changes data outside the product), the spec
  must say what happens on success, on a definite rejection, on an unknown result (a
  time-out, a lost response or a crash), and on retry. A missing outcome that can
  change correctness or safety is P2.
- A spec about input that the product does not control, about concurrency or about
  crash recovery must have a failure list (`## Failure list`). A spec with no failure
  list for such a subject is P2. One more entry for the list is P3.
- A finding that the spec does not contain X blocks only when X is a decision that the
  design needs in order to be safe.
