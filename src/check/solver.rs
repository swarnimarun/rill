//! Private rank-one equality solver. Public checker types remain tree-shaped;
//! immutable interned terms and union-find variables are confined to this module.
//!
//! Parent traversal, normalization, substitution, unification, occurs checking
//! and level lowering use explicit worklists. This removes recursion on variable
//! links, not the public type tree's separate size and destruction constraints.
use super::{
    type_cursor::{TypeChildren, TypeCursor, TypeHead},
    Type,
};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

type TermId = usize;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Term {
    Variable(usize),
    // Only non-Var, non-Nominal public types are atoms.
    Atom(Type),
    Nominal(String, Vec<TermId>),
    Callable(Vec<crate::ast::ParamMode>, Vec<TermId>),
}

struct Variable {
    parent: Cell<usize>,
    rank: u32,
    level: usize,
    term: TermId,
    // Representatives are unbound or bound to a non-variable term.
    binding: Option<TermId>,
}

#[derive(Default)]
pub(super) struct Solver {
    variables: Vec<Variable>,
    terms: Vec<Term>,
    interned: HashMap<Term, TermId>,
    materialization: materialize::Budget,
}

#[derive(Clone, Copy)]
enum Input<'a> {
    Public(&'a Type),
    Private(TermId),
}

enum View<'a> {
    Variable(usize),
    Atom,
    PublicNominal(&'a [Type]),
    PrivateNominal(&'a [TermId]),
    PublicCallable(TypeChildren<'a>),
}

impl Solver {
    fn intern(&mut self, term: Term) -> TermId {
        if let Some(id) = self.interned.get(&term) {
            return *id;
        }
        let id = self.terms.len();
        self.terms.push(term.clone());
        self.interned.insert(term, id);
        id
    }

    pub(super) fn fresh(&mut self, level: usize) -> Type {
        let id = self.variables.len();
        let term = self.intern(Term::Variable(id));
        self.variables.push(Variable {
            parent: Cell::new(id),
            rank: 0,
            level,
            term,
            binding: None,
        });
        Type::Var(id)
    }

    /// Full iterative compression changes only representation, not equivalence.
    fn representative(&self, id: usize) -> usize {
        let mut root = id;
        while self.variables[root].parent.get() != root {
            root = self.variables[root].parent.get();
        }
        let mut current = id;
        while current != root {
            current = self.variables[current].parent.replace(root);
        }
        root
    }

    pub(super) fn level(&self, id: usize) -> usize {
        self.variables[self.representative(id)].level
    }

    pub(super) fn lower_variable(&mut self, id: usize, level: usize) {
        let root = self.representative(id);
        self.variables[root].level = self.variables[root].level.min(level);
    }

    fn head(&self, mut term: TermId) -> TermId {
        while let Term::Variable(id) = self.terms[term] {
            let variable = &self.variables[self.representative(id)];
            match variable.binding {
                Some(binding) => term = binding,
                None => return variable.term,
            }
        }
        term
    }

    /// Read-only bounded cursor for source coverage. This path deliberately does
    /// not call representative(), head() or view(): even path compression would
    /// hide uncharged work and could mutate parents before a callback failure.
    ///
    /// Charge separately before each public/arena head, each variable's parent
    /// lookup (including the root), and the root's binding lookup. Child slices
    /// are borrowed, never expanded; the consumer must charge before scheduling.
    pub(super) fn type_head<'a>(
        &'a self,
        mut cursor: TypeCursor<'a>,
        mut charge: impl FnMut() -> Result<(), String>,
    ) -> Result<TypeHead<'a>, String> {
        loop {
            charge()?;
            let mut id = match cursor {
                TypeCursor::Public(Type::Var(id)) => *id,
                TypeCursor::Public(Type::Nominal(name, children)) => {
                    return Ok(TypeHead::Nominal(name, TypeChildren::Public(children)));
                }
                TypeCursor::Public(Type::Callable {
                    environment,
                    params,
                    modes,
                    result,
                }) => {
                    return Ok(TypeHead::Callable(
                        modes,
                        TypeChildren::Callable {
                            environment,
                            params,
                            result,
                        },
                    ));
                }
                TypeCursor::Public(atom) => return Ok(TypeHead::Atom(atom)),
                TypeCursor::Private(term) => match &self.terms[term] {
                    Term::Variable(id) => *id,
                    Term::Atom(atom) => return Ok(TypeHead::Atom(atom)),
                    Term::Nominal(name, children) => {
                        return Ok(TypeHead::Nominal(name, TypeChildren::Private(children)));
                    }
                    Term::Callable(modes, children) => {
                        return Ok(TypeHead::Callable(modes, TypeChildren::Private(children)));
                    }
                },
            };
            loop {
                charge()?;
                let parent = self.variables[id].parent.get();
                if parent == id {
                    break;
                }
                id = parent;
            }
            charge()?;
            match self.variables[id].binding {
                Some(term) => cursor = TypeCursor::Private(term),
                None => return Ok(TypeHead::Variable(id)),
            }
        }
    }

    fn view<'a>(&'a self, input: Input<'a>) -> View<'a> {
        // Public variables enter the arena once; nominal children stay lazy.
        let input = match input {
            Input::Public(Type::Var(id)) => Input::Private(self.variables[*id].term),
            other => other,
        };
        match input {
            Input::Public(Type::Nominal(_, arguments)) => View::PublicNominal(arguments),
            Input::Public(Type::Callable {
                environment,
                params,
                result,
                ..
            }) => View::PublicCallable(TypeChildren::Callable {
                environment,
                params,
                result,
            }),
            Input::Public(_) => View::Atom,
            Input::Private(term) => match &self.terms[self.head(term)] {
                Term::Variable(id) => View::Variable(*id),
                Term::Atom(_) => View::Atom,
                Term::Nominal(_, arguments) => View::PrivateNominal(arguments),
                Term::Callable(_, arguments) => View::PrivateNominal(arguments),
            },
        }
    }

    fn free(&self, input: Input<'_>, out: &mut HashSet<usize>) {
        let mut work = vec![input];
        let mut visited = HashSet::new();
        while let Some(input) = work.pop() {
            let input = match input {
                Input::Public(Type::Var(id)) => Input::Private(self.variables[*id].term),
                other => other,
            };
            if let Input::Private(term) = input {
                if !visited.insert(self.head(term)) {
                    continue;
                }
            }
            match self.view(input) {
                View::Variable(id) => {
                    out.insert(id);
                }
                View::PublicNominal(arguments) => work.extend(arguments.iter().map(Input::Public)),
                View::PublicCallable(children) => {
                    for index in 0..children.len() {
                        let Some(TypeCursor::Public(ty)) = children.get(index) else {
                            unreachable!()
                        };
                        work.push(Input::Public(ty));
                    }
                }
                View::PrivateNominal(arguments) => {
                    work.extend(arguments.iter().map(|t| Input::Private(*t)))
                }
                View::Atom => {}
            }
        }
    }

    pub(super) fn free_type(&self, ty: &Type, out: &mut HashSet<usize>) {
        self.free(Input::Public(ty), out);
    }

    fn import(&mut self, ty: Type) -> TermId {
        enum Work {
            Visit(Type),
            Build(String, usize),
            BuildCallable(Vec<crate::ast::ParamMode>, usize),
        }
        let mut work = vec![Work::Visit(ty)];
        let mut values = Vec::new();
        while let Some(step) = work.pop() {
            let term = match step {
                Work::Build(name, arity) => {
                    let arguments = values.split_off(values.len() - arity);
                    Term::Nominal(name, arguments)
                }
                Work::BuildCallable(modes, arity) => {
                    Term::Callable(modes, values.split_off(values.len() - arity))
                }
                Work::Visit(Type::Callable {
                    environment,
                    params,
                    modes,
                    result,
                }) => {
                    work.push(Work::BuildCallable(modes, params.len() + 2));
                    work.push(Work::Visit(*result));
                    work.extend(params.into_iter().rev().map(Work::Visit));
                    work.push(Work::Visit(*environment));
                    continue;
                }
                Work::Visit(Type::Nominal(name, arguments)) => {
                    work.push(Work::Build(name, arguments.len()));
                    work.extend(arguments.into_iter().rev().map(Work::Visit));
                    continue;
                }
                Work::Visit(Type::Var(id)) => {
                    values.push(self.variables[self.representative(id)].term);
                    continue;
                }
                Work::Visit(atom) => Term::Atom(atom),
            };
            values.push(self.intern(term));
        }
        debug_assert_eq!(values.len(), 1);
        values.pop().unwrap()
    }

    fn union(&mut self, mut left: usize, mut right: usize) {
        debug_assert!(self.variables[left].binding.is_none());
        debug_assert!(self.variables[right].binding.is_none());
        if self.variables[left].rank < self.variables[right].rank
            || (self.variables[left].rank == self.variables[right].rank && left > right)
        {
            std::mem::swap(&mut left, &mut right);
        }
        self.variables[right].parent.set(left);
        self.variables[left].level = self.variables[left].level.min(self.variables[right].level);
        if self.variables[left].rank == self.variables[right].rank {
            self.variables[left].rank += 1;
        }
    }

    fn bind(&mut self, variable: usize, term: TermId) -> Result<(), String> {
        let mut free = HashSet::new();
        self.free(Input::Private(term), &mut free);
        if free.contains(&variable) {
            return Err("E-TYPE: infinite type".into());
        }
        // Finish the occurs check before any level mutation.
        let level = self.variables[variable].level;
        for id in free {
            self.lower_variable(id, level);
        }
        self.variables[variable].binding = Some(term);
        Ok(())
    }

    pub(super) fn unify(&mut self, left: Type, right: Type) -> Result<(), String> {
        let left = self.import(left);
        let right = self.import(right);
        let mut work = vec![(left, right)];
        let mut compared = HashSet::new();
        while let Some((left, right)) = work.pop() {
            let left = self.head(left);
            let right = self.head(right);
            if left == right || !compared.insert((left.min(right), left.max(right))) {
                continue;
            }
            match (&self.terms[left], &self.terms[right]) {
                (Term::Variable(a), Term::Variable(b)) => self.union(*a, *b),
                (Term::Variable(id), _) => self.bind(*id, right)?,
                (_, Term::Variable(id)) => self.bind(*id, left)?,
                (Term::Nominal(a, aa), Term::Nominal(b, bb)) if a == b && aa.len() == bb.len() => {
                    // Keep left-to-right error ordering while using a LIFO stack.
                    work.extend(aa.iter().zip(bb).rev().map(|(a, b)| (*a, *b)));
                }
                (Term::Callable(a, aa), Term::Callable(b, bb))
                    if a == b && aa.len() == bb.len() =>
                {
                    work.extend(aa.iter().zip(bb).rev().map(|(a, b)| (*a, *b)));
                }
                _ => {
                    // A mismatch must not expand or recursively format a potentially
                    // enormous solved DAG merely to report that it is incompatible.
                    return Err("E-TYPE: incompatible types".into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cursor_tests;

mod materialize;
