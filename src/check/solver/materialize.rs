//! Bounded public-tree construction. Weight is a deterministic policy unit,
//! not a measurement of allocator bytes or a whole-compiler memory bound.
use super::*;

pub(super) const LIMIT_ERROR: &str =
    "E-TYPE-COMPLEXITY: type materialization budget exceeded; simplify the program";
const NODE_WEIGHT: usize = 64;
const TREE_LIMIT: usize = 16 * 1024 * 1024;
const TOTAL_LIMIT: usize = 64 * 1024 * 1024;
const DEPTH_LIMIT: usize = 128;
const WORK_LIMIT: usize = 4_000_000;

pub(super) struct Budget {
    tree_limit: usize,
    depth_limit: usize,
    remaining: Cell<usize>,
    work: Cell<usize>,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            tree_limit: TREE_LIMIT,
            depth_limit: DEPTH_LIMIT,
            remaining: Cell::new(TOTAL_LIMIT),
            work: Cell::new(WORK_LIMIT),
        }
    }
}
impl Budget {
    fn work(&self, count: usize) -> Result<(), String> {
        self.work
            .set(self.work.get().checked_sub(count).ok_or(LIMIT_ERROR)?);
        Ok(())
    }
    fn reserve(&self, weight: usize) -> Result<(), String> {
        if weight > self.tree_limit {
            return Err(LIMIT_ERROR.into());
        }
        self.remaining.set(
            self.remaining
                .get()
                .checked_sub(weight)
                .ok_or(LIMIT_ERROR)?,
        );
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Source<'a> {
    Solved(TypeCursor<'a>),
    // A replacement is copied verbatim, not normalized or substituted again.
    Raw(&'a Type),
}
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum Key {
    Public(*const Type),
    Private(TermId),
    Raw(*const Type),
}
impl Source<'_> {
    fn key(self) -> Key {
        match self {
            Self::Solved(TypeCursor::Public(ty)) => Key::Public(ty),
            Self::Solved(TypeCursor::Private(id)) => Key::Private(id),
            Self::Raw(ty) => Key::Raw(ty),
        }
    }
}

enum Head<'a> {
    Variable(usize),
    Atom(&'a Type),
    Nominal(&'a str, Children<'a>),
    Replacement(&'a Type),
}
enum Children<'a> {
    Solved(TypeChildren<'a>),
    Raw(&'a [Type]),
}
impl<'a> Children<'a> {
    fn len(&self) -> usize {
        match self {
            Self::Solved(children) => children.len(),
            Self::Raw(children) => children.len(),
        }
    }
    fn get(&self, index: usize) -> Source<'a> {
        match self {
            Self::Solved(children) => Source::Solved(children.get(index).unwrap()),
            Self::Raw(children) => Source::Raw(&children[index]),
        }
    }
}

#[derive(Clone, Copy)]
struct Size {
    weight: usize,
    depth: usize,
}
impl Size {
    fn node(name_bytes: usize) -> Result<Self, String> {
        Ok(Self {
            weight: NODE_WEIGHT.checked_add(name_bytes).ok_or(LIMIT_ERROR)?,
            depth: 1,
        })
    }
    fn add_child(&mut self, child: Self) -> Result<(), String> {
        self.weight = self.weight.checked_add(child.weight).ok_or(LIMIT_ERROR)?;
        self.depth = self
            .depth
            .max(child.depth.checked_add(1).ok_or(LIMIT_ERROR)?);
        Ok(())
    }
}

struct Node<'a> {
    size: Size,
    head: Head<'a>,
}

impl Solver {
    fn materialization_head<'a>(
        &'a self,
        source: Source<'a>,
        replacements: &'a HashMap<usize, Type>,
    ) -> Result<Head<'a>, String> {
        match source {
            Source::Raw(ty) => {
                self.materialization.work(1)?;
                Ok(match ty {
                    Type::Nominal(name, children) => Head::Nominal(name, Children::Raw(children)),
                    atom => Head::Atom(atom),
                })
            }
            Source::Solved(cursor) => {
                let head = self.type_head(cursor, || self.materialization.work(1))?;
                Ok(match head {
                    TypeHead::Variable(id) => match replacements.get(&id) {
                        Some(ty) => Head::Replacement(ty),
                        None => Head::Variable(id),
                    },
                    TypeHead::Atom(atom) => Head::Atom(atom),
                    TypeHead::Nominal(name, children) => {
                        Head::Nominal(name, Children::Solved(children))
                    }
                })
            }
        }
    }

    /// Postorder summaries memoize arena handles and raw replacement addresses.
    /// Every parent adds the child's entire expanded weight, even on cache hits.
    /// No output Type or owned name is allocated until the complete preflight
    /// succeeds. Failure drops only flat borrowed worklists and scalar summaries.
    fn preflight<'a>(
        &'a self,
        source: Source<'a>,
        replacements: &'a HashMap<usize, Type>,
    ) -> Result<HashMap<Key, Node<'a>>, String> {
        enum Work<'a> {
            Visit(Source<'a>, usize),
            Finish(Key, Size, Head<'a>),
            Alias(Key, &'a Type),
        }
        let mut memo = HashMap::<Key, Node<'_>>::new();
        let mut work = vec![Work::Visit(source, 1)];
        let mut sizes: Vec<Size> = Vec::new();
        while let Some(step) = work.pop() {
            self.materialization.work(1)?;
            let (key, size, head) = match step {
                Work::Finish(key, mut size, head) => {
                    let Head::Nominal(_, children) = &head else {
                        unreachable!()
                    };
                    let count = children.len();
                    for child in sizes.drain(sizes.len() - count..) {
                        size.add_child(child)?;
                        if size.weight > self.materialization.tree_limit {
                            return Err(LIMIT_ERROR.into());
                        }
                    }
                    (key, size, head)
                }
                Work::Alias(key, ty) => (key, sizes.pop().unwrap(), Head::Replacement(ty)),
                Work::Visit(source, depth) => {
                    if depth > self.materialization.depth_limit {
                        return Err(LIMIT_ERROR.into());
                    }
                    let key = source.key();
                    if let Some(node) = memo.get(&key) {
                        let size = node.size;
                        if size.depth > self.materialization.depth_limit.saturating_sub(depth - 1) {
                            return Err(LIMIT_ERROR.into());
                        }
                        sizes.push(size);
                        continue;
                    }
                    let head = self.materialization_head(source, replacements)?;
                    let size = match &head {
                        Head::Variable(_) => Size::node(0)?,
                        Head::Atom(Type::CPointer { pointee, .. }) => Size::node(pointee.len())?,
                        Head::Atom(_) => Size::node(0)?,
                        Head::Replacement(ty) => {
                            work.push(Work::Alias(key, ty));
                            work.push(Work::Visit(Source::Raw(ty), depth));
                            continue;
                        }
                        Head::Nominal(name, children) => {
                            let size = Size::node(name.len())?;
                            let count = children.len();
                            // Reject impossible width before scheduling/allocating it.
                            let minimum = count
                                .checked_mul(NODE_WEIGHT)
                                .and_then(|w| w.checked_add(size.weight))
                                .ok_or(LIMIT_ERROR)?;
                            if minimum > self.materialization.tree_limit {
                                return Err(LIMIT_ERROR.into());
                            }
                            self.materialization.work(count)?;
                            let next_depth = depth.checked_add(1).ok_or(LIMIT_ERROR)?;
                            let start = work.len();
                            for index in (0..count).rev() {
                                work.push(Work::Visit(children.get(index), next_depth));
                            }
                            work.insert(start, Work::Finish(key, size, head));
                            continue;
                        }
                    };
                    (key, size, head)
                }
            };
            if size.weight > self.materialization.tree_limit
                || size.depth > self.materialization.depth_limit
            {
                return Err(LIMIT_ERROR.into());
            }
            memo.insert(key, Node { size, head });
            sizes.push(size);
        }
        debug_assert_eq!(sizes.len(), 1);
        Ok(memo)
    }

    fn materialize(
        &self,
        source: Source<'_>,
        replacements: &HashMap<usize, Type>,
    ) -> Result<Type, String> {
        let plan = self.preflight(source, replacements)?;
        self.materialization
            .reserve(plan[&source.key()].size.weight)?;
        enum Work<'a> {
            Visit(Source<'a>),
            Build(&'a str, usize),
        }
        let mut work = vec![Work::Visit(source)];
        let mut values = Vec::new();
        // No fallible budget operations after reservation: every output node,
        // copied replacement and name has already been counted with multiplicity.
        while let Some(step) = work.pop() {
            match step {
                Work::Build(name, arity) => {
                    let arguments = values.split_off(values.len() - arity);
                    values.push(Type::Nominal(name.to_owned(), arguments));
                }
                Work::Visit(source) => match &plan[&source.key()].head {
                    Head::Variable(id) => values.push(Type::Var(*id)),
                    Head::Atom(atom) => values.push((*atom).clone()),
                    Head::Replacement(ty) => work.push(Work::Visit(Source::Raw(ty))),
                    Head::Nominal(name, children) => {
                        work.push(Work::Build(name, children.len()));
                        for index in (0..children.len()).rev() {
                            work.push(Work::Visit(children.get(index)));
                        }
                    }
                },
            }
        }
        debug_assert_eq!(values.len(), 1);
        Ok(values.pop().unwrap())
    }

    pub(in crate::check) fn resolve(&self, ty: &Type) -> Result<Type, String> {
        self.materialize(Source::Solved(TypeCursor::Public(ty)), &HashMap::new())
    }

    pub(in crate::check) fn substitute(
        &self,
        ty: &Type,
        replacements: &HashMap<usize, Type>,
    ) -> Result<Type, String> {
        self.materialize(Source::Solved(TypeCursor::Public(ty)), replacements)
    }

    pub(in crate::check) fn copy_type(&self, ty: &Type) -> Result<Type, String> {
        self.materialize(Source::Raw(ty), &HashMap::new())
    }
}

#[cfg(test)]
mod tests;
