use rill::{check, parser};
use std::collections::{BTreeMap, HashMap};
fn canonical(ty: &check::Type, ids: &mut HashMap<usize, usize>) -> check::Type {
    match ty {
        check::Type::Var(id) => {
            let next = ids.len();
            check::Type::Var(*ids.entry(*id).or_insert(next))
        }
        check::Type::Nominal(name, args) => check::Type::Nominal(
            name.clone(),
            args.iter().map(|t| canonical(t, ids)).collect(),
        ),
        atom => atom.clone(),
    }
}
pub fn canonical_signature(
    signature: &check::FunctionType,
) -> (Vec<usize>, Vec<check::Type>, check::Type) {
    let mut ids = HashMap::new();
    let params = signature
        .params
        .iter()
        .map(|ty| canonical(ty, &mut ids))
        .collect();
    let result = canonical(&signature.result, &mut ids);
    // Preserve membership, including quantified variables absent from the
    // parameter/result tree, rather than only comparing quantifier counts.
    let mut quantified: Vec<_> = signature
        .quantified
        .iter()
        .map(|id| {
            let next = ids.len();
            *ids.entry(*id).or_insert(next)
        })
        .collect();
    quantified.sort_unstable();
    (quantified, params, result)
}
pub fn fingerprints() -> String {
    let corpus = [
        "fn identity(x){x}\nfn main(){identity(42)}",
        "type O<a> = None | Some(a)\nfn absent(){None}\nfn main(){let none=None;let one=if true{none}else{Some(1)};if true{none}else{Some(true)}}",
        "type P<a,b> = P(a,b)\nfn pair(a,b){P(a,b)}\nfn duplicate(x){pair(x,x)}\nfn main(){duplicate(42)}",
        "fn f(x){g(x)}\nfn g(x){if true{f(x)}else{x}}\nfn main(){f(42)}",
        "fn g(x){if true{f(x)}else{x}}\nfn f(x){g(x)}\nfn main(){f(42)}",
        "type P = {x:i32}\nfn f(x){let p=g(x);p.x}\nfn g(x:P){if true{x}else{record P{x=f(x)}}}\nfn main(){42}",
        "type P = {x:i32}\nfn g(x:P){if true{x}else{record P{x=f(x)}}}\nfn f(x){let p=g(x);p.x}\nfn main(){42}",
        "type O<a> = None|Some(a)\nfn main(){let mut x=None;let y=x;x<-Some(1);if true{y}else{Some(true)}}",
        "type W<a> = W(a)\nfn f(x){g(W(x))}\nfn g(x){f(x)}\nfn main(){42}",
        "fn main(){c_null()}",
        "fn inherited(x:ptr<SystemBytes>){x}\nfn main(){42}",
        "fn plain(x:ptr<Opaque>){x}\nfn main(){42}",
        "let identity x = x\nlet main = identity(42)",
        "fn booleans(x:bool){x}\nfn unused(x,y){x}\nfn main(){if booleans(true){42}else{0}}",
        "fn a(x){b(x)}\nfn b(x){c(x)}\nfn c(x){if true{a(x)}else{x}}\nfn main(){a(42)}",
        "type O<a> = None|Some(a)\nlet main = let mut cell = None in let alias = cell in let changed = cell <- Some(1) in if true then alias else Some(true)",
    ];
    let mut output = String::new();
    for (index, source) in corpus.iter().enumerate() {
        let program = parser::parse(source).unwrap();
        match check::analyze(&program) {
            Ok(checked) => {
                let signatures: BTreeMap<_, _> = program
                    .functions
                    .iter()
                    .map(|f| {
                        let signature = &checked.functions[&f.name];
                        (&f.name, canonical_signature(signature))
                    })
                    .collect();
                output.push_str(&format!("{index}: {signatures:?}\n"));
            }
            Err(error) => {
                output.push_str(&format!("{index}: {}\n", error.split(':').next().unwrap()))
            }
        }
    }
    output
}
