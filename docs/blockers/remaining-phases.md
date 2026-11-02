# Remaining implementation

`PLAN.md` describes a full language, compiler, libraries, tools, native and C
interoperability, formal work, and release process. This repository began with
only that document. The current implementation is an initial subset, not a
stable language or a completed implementation of the plan.

The phase gates in section 19 remain authoritative. In particular, P1 needs a
complete typed sequential kernel, generic specialization and compact ADT
layouts for C output, and broader interpreter/C differential evidence. See
[c-adt-layout.md](c-adt-layout.md) for the immediate C backend constraint. P2 through P11 need further
ownership, effects, modules, interoperability, runtime, tooling, optimization,
and release work. The open decisions in section 22 also need resolution before
the affected contracts can be called stable. These are outstanding engineering
work and design decisions, not evidence that any omitted feature is supported.
