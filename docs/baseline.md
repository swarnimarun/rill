# Baseline and phase status

The working tree supplied for this implementation contained the proposed
language and compiler blueprint in `PLAN.md`, with no compiler, examples or
baseline test suite in that tree. The repository also retains an older,
separate implementation history on `main`, discovered while preparing the
`v1` bookmark. This checkpoint has not compared or migrated that implementation.
The blueprint
is the source of the phase gates and design constraints, not evidence that those
gates have been met.

The implementation starts with a sequential subset and C output. Integers are
32-bit signed values; arithmetic overflow traps. Evaluation is strict and
left-to-right. Unsupported language features must be rejected rather than
silently approximated. Ownership, effects and migration decisions remain as
specified or open in the blueprint, and have not yet been validated by a full
implementation.

P0 is partial: this baseline records the available source and absence of tests,
but the required normative examples and decision records need review. P1 and
all later phases remain open until their acceptance evidence in section 19 of
`PLAN.md` exists. In particular, a working subset does not establish the HM,
ADT, interpreter parity, ownership, effects, interoperability, or stability
claims in that plan.
