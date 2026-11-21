# Health and shield

This example models a hit against a character. Damage first consumes shield;
any remainder reduces health, which is clamped at zero. The record starts with
100 health and 30 shield. A hit of 45 leaves 85 health and no shield.

From the repository root:

```sh
cargo run -- run examples/records/damage.rill
cargo run -- build examples/records/damage.rill -o /tmp/rill-damage
/tmp/rill-damage
```

Each command that runs the program prints `85`. The hit routine borrows
`status.health` and `status.shield` exclusively as disjoint fields. Its inputs
are assumed nonnegative; it is a small arithmetic example, not a complete game
state or error-handling API. Record values currently use Copy semantics, and
field loans use copy-in/writeback, which may copy the containing value. See
[ownership limits](../../docs/blockers/ownership.md).
