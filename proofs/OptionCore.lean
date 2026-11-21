-- Lean 3.51.0, bundled core only.
namespace Rill.OptionCore
inductive Ty
| i32 | bool | opt : Ty → Ty

-- Each 32-bit bit pattern is a distinct signed integer literal.
-- The ordinal of the two's-complement bits. The core has no arithmetic.
def I32 := fin 4294967296
inductive Tm
| int : I32 → Tm
| bool : bool → Tm
| none : Tm
| some : Tm → Tm
| branch : Tm → Tm → Tm → Tm
| getD : Tm → Tm → Tm

inductive Has : Tm → Ty → Prop
| int (n) : Has (Tm.int n) Ty.i32
| bool (b) : Has (Tm.bool b) Ty.bool
| none {a} : Has Tm.none (Ty.opt a)
| some {e a} : Has e a → Has (Tm.some e) (Ty.opt a)
| branch {c y n a} : Has c Ty.bool → Has y a → Has n a → Has (Tm.branch c y n) a
| getD {s d a} : Has s (Ty.opt a) → Has d a → Has (Tm.getD s d) a

inductive Checked : Ty → Type
| int (n : I32) : Checked Ty.i32
| bool (b : bool) : Checked Ty.bool
| none {a} : Checked (Ty.opt a)
| some {a} : Checked a → Checked (Ty.opt a)
| branch {a} : Checked Ty.bool → Checked a → Checked a → Checked a
| getD {a} : Checked (Ty.opt a) → Checked a → Checked a

def erase : Π {a}, Checked a → Tm
| _ (Checked.int n) := Tm.int n
| _ (Checked.bool b) := Tm.bool b
| _ Checked.none := Tm.none
| _ (Checked.some e) := Tm.some (erase e)
| _ (Checked.branch c y n) := Tm.branch (erase c) (erase y) (erase n)
| _ (Checked.getD s d) := Tm.getD (erase s) (erase d)

theorem erasureTyped {a} (e : Checked a) : Has (erase e) a :=
by induction e; constructor; assumption

inductive Value : Tm → Prop
| int (n) : Value (Tm.int n)
| bool (b) : Value (Tm.bool b)
| none : Value Tm.none
| some {e} : Value e → Value (Tm.some e)

-- Only the condition or scrutinee steps before selecting a branch.
-- Some evaluates its argument before returning the payload.
inductive Step : Tm → Tm → Prop
| some {e u} : Step e u → Step (Tm.some e) (Tm.some u)
| cond {c c' y n} : Step c c' → Step (Tm.branch c y n) (Tm.branch c' y n)
| yes {y n} : Step (Tm.branch (Tm.bool tt) y n) y
| no {y n} : Step (Tm.branch (Tm.bool ff) y n) n
| scrut {s s' d} : Step s s' → Step (Tm.getD s d) (Tm.getD s' d)
| present {v d} : Value v → Step (Tm.getD (Tm.some v) d) v
| absent {d} : Step (Tm.getD Tm.none d) d

theorem valueNormal {e} (v : Value e) : ∀ other, ¬ Step e other :=
begin
  induction v with _ _ _ _ ih; intros other h; cases h,
  apply ih _; assumption
end

theorem canonicalBool {e} (v : Value e) (ty : Has e Ty.bool) :
  e = Tm.bool tt ∨ e = Tm.bool ff :=
begin
  cases v with n b e h,
  { cases ty },
  { cases b,
    { exact or.inr rfl },
    { exact or.inl rfl } },
  { cases ty },
  { cases ty }
end

theorem canonicalOption {e a} (v : Value e) (ty : Has e (Ty.opt a)) :
  e = Tm.none ∨ ∃ arg, e = Tm.some arg ∧ Value arg :=
begin
  cases v with n b e h,
  { cases ty },
  { cases ty },
  { exact or.inl rfl },
  { exact or.inr ⟨_, rfl, h⟩ }
end

theorem progress {e a} (ty : Has e a) : Value e ∨ ∃ u, Step e u :=
begin
  induction ty with n b a e a h ih c y n a hc hy hn ic iy inn s d a hs hd is id,
  { exact or.inl (Value.int n) },
  { exact or.inl (Value.bool b) },
  { exact or.inl Value.none },
  { cases ih with hv poss,
    { exact or.inl (Value.some hv) },
    { cases poss with u h, exact or.inr ⟨_, Step.some h⟩ } },
  { cases ic with hv poss,
    { cases canonicalBool hv hc with h h; subst c,
      { exact or.inr ⟨_, Step.yes⟩ },
      { exact or.inr ⟨_, Step.no⟩ } },
    { cases poss with u h, exact or.inr ⟨_, Step.cond h⟩ } },
  { cases is with hv poss,
    { cases canonicalOption hv hs with h some_,
      { subst s, exact or.inr ⟨_, Step.absent⟩ },
      { cases some_ with arg p, cases p with h v, subst s,
        exact or.inr ⟨_, Step.present v⟩ } },
    { cases poss with u h, exact or.inr ⟨_, Step.scrut h⟩ } }
end

theorem preservation : ∀ {e u a}, Step e u → Has e a → Has u a
| _ _ _ (Step.some s) (Has.some h) := Has.some (preservation s h)
| _ _ _ (Step.cond s) (Has.branch c y n) := Has.branch (preservation s c) y n
| _ _ _ Step.yes (Has.branch c y n) := y
| _ _ _ Step.no (Has.branch c y n) := n
| _ _ _ (Step.scrut s) (Has.getD h d) := Has.getD (preservation s h) d
| _ _ _ (Step.present _) (Has.getD (Has.some h) _) := h
| _ _ _ Step.absent (Has.getD _ d) := d

inductive Steps : Tm → Tm → Prop
| refl {e} : Steps e e
| cons {e mid last} : Step e mid → Steps mid last → Steps e last

theorem reachableTyping {e last a} (steps : Steps e last) : Has e a → Has last a :=
begin
  induction steps with e e mid last first rest ih; intro h,
  { exact h },
  { exact ih (preservation first h) }
end

theorem reachableProgress {e last a} (steps : Steps e last) (ty : Has e a) :
  Value last ∨ ∃ u, Step last u :=
progress (reachableTyping steps ty)

-- The Rill tests instantiate both traces at n = 42 and d = 7.
example (n d : I32) :
  Steps (Tm.getD (Tm.branch (Tm.bool tt) (Tm.some (Tm.int n)) Tm.none) (Tm.int d)) (Tm.int n) :=
Steps.cons (Step.scrut Step.yes) (Steps.cons (Step.present (Value.int n)) Steps.refl)

example (n d : I32) :
  Steps (Tm.getD (Tm.branch (Tm.bool ff) (Tm.some (Tm.int n)) Tm.none) (Tm.int d)) (Tm.int d) :=
Steps.cons (Step.scrut Step.no) (Steps.cons Step.absent Steps.refl)

#print axioms erasureTyped
#print axioms valueNormal
#print axioms progress
#print axioms preservation
#print axioms reachableProgress
end Rill.OptionCore
