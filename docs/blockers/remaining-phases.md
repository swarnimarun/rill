# Remaining implementation

`PLAN.md` describes a full language, compiler, libraries, tools, native and C
interoperability, formal work, and release process. This repository began with
only that document. The current implementation is an initial subset, not a
stable language or a completed implementation of the plan.

Use [phase status](../phase-status.md) for current capabilities and evidence,
[the active roadmap](../CONTINUATION_PLAN.md) for work order, and
[build performance](../build-performance.md) for measurement and optimization
gates. This page summarizes remaining contracts; it is not a separate backlog.

The phase gates in section 19 remain authoritative. In particular, P1 needs a
complete typed sequential kernel, complete representation coverage for the
accepted subset and broader
interpreter/C differential evidence. Concrete generic functions and finite
Copy enums, records, root/field loans and existing Copy vectors now share the
independently checked typed interpreter/C path; these bounded increments do
not complete P1. See
[c-adt-layout.md](c-adt-layout.md) for the immediate C backend constraint. P2 through P11 need further
ownership, effects, modules, interoperability, runtime, tooling, optimization,
and release work. The open decisions in section 22 also need resolution before
the affected contracts can be called stable. These are outstanding engineering
work and design decisions, not evidence that any omitted feature is supported.
