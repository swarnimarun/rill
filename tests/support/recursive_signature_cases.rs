//! Closed parameter preconstraints, not general projection-constraint inference.
pub fn cases() -> Vec<(String, String, &'static str)> {
    let mut cases = Vec::new();
    for (label, first, second, main, expected) in [
        ("defining", "fn f(p)->i32{g(p);p.x}", "fn g(p:P)->i32{if false{f(p)}else{0}}", "fn main(){f(record P{x=42})}", "42"),
        ("mixed", "fn f(p)->i32{let q=g(p);p.x+q.x}", "fn g(p:P)->P{if false{record P{x=f(p)}}else{p}}", "fn main(){f(record P{x=21})}", "42"),
        ("read-forward", "fn f(read p)->i32{g(p);p.x}", "fn g(read p:P)->i32{if false{f(p)}else{0}}", "fn main(){f(record P{x=42})}", "42"),
        ("inout-forward", "fn f(inout p)->i32{g(inout p);p.x}", "fn g(inout p:P)->unit{if false{f(inout p);}else{p.x<-p.x+1;}}", "fn main(){let mut p=record P{x=41};let answer=f(inout p);answer+p.x}", "84"),
        ("value-snapshot", "fn f(before,inout p)->i32{g(before,inout p);before.x*100+p.x}", "fn g(before:P,inout p:P)->unit{if false{f(before,inout p);}else{p.x<-p.x+1;}}", "fn main(){let mut p=record P{x=1};f(p,inout p)}", "102"),
        ("computed-read-snapshot", "fn f(read before,inout p)->i32{g(before,inout p);before.x*100+p.x}", "fn g(read before:P,inout p:P)->unit{if false{f(before,inout p);}else{p.x<-p.x+1;}}", "fn main(){let mut p=record P{x=1};f({record P{x=p.x}},inout p)}", "102"),
        ("written-effects", "fn f(read a,read b)->i32{g(a,b);a.x*10+b.x}", "fn g(read a:P,read b:P)->unit{if false{f(a,b);}else{()}}", "fn main(){let mut n=0;let answer=f({n<-n+1;record P{x=n}},{n<-n+1;record P{x=n}});answer*10+n}", "122"),
        ("legacy", "let f p = let ignored=g(p) in p.x", "let g (p:P) = if false then f(p) else 0", "let main = f(record P{x=42})", "42"),
    ] {
        for (order, declarations) in [format!("{first}\n{second}"), format!("{second}\n{first}")].into_iter().enumerate() {
            cases.push((format!("{label}-{order}"), format!("type P={{x:i32}}\n{declarations}\n{main}"), expected));
        }
    }
    let declarations = [
        "fn a(p)->i32{b(p);p.x}",
        "fn b(p:P)->i32{c(p)}",
        "fn c(p:P)->i32{if false{a(p)}else{0}}",
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        cases.push((
            format!("three-{}{}{}", order[0], order[1], order[2]),
            format!(
                "type P={{x:i32}}\n{}\n{}\n{}\nfn main(){{a(record P{{x=42}})}}",
                declarations[order[0]], declarations[order[1]], declarations[order[2]]
            ),
            "42",
        ));
    }
    cases
}
