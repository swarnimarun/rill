//! Shared, bounded nominal-projection fixtures. Expected outcomes are assertions,
//! not a record of checks performed by the author of these fixtures.
pub struct Case {
    pub label: String,
    pub source: String,
    pub expected: Option<&'static str>,
    pub reference_c: bool,
}

fn add(cases: &mut Vec<Case>, label: &str, source: impl Into<String>, expected: &'static str) {
    cases.push(Case {
        label: label.into(),
        source: source.into(),
        expected: Some(expected),
        reference_c: true,
    });
}

fn orders(
    cases: &mut Vec<Case>,
    label: &str,
    types: &str,
    functions: [&str; 2],
    main: &str,
    expected: &'static str,
) {
    for (order, [first, second]) in [functions, [functions[1], functions[0]]]
        .into_iter()
        .enumerate()
    {
        add(
            cases,
            &format!("{label}-{order}"),
            format!("{types}\n{first}\n{second}\n{main}"),
            expected,
        );
    }
}

pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    orders(
        &mut cases,
        "unannotated-result",
        "type P={x:i32}",
        [
            "fn f(x){let p=g(x);p.x}",
            "fn g(x:P){if true{x}else{record P{x=f(x)}}}",
        ],
        "fn main(){f(record P{x=42})}",
        "42",
    );
    let functions = [
        "fn f(p)->i32{g(p);p.x}",
        "fn g(p)->i32{h(p)}",
        "fn h(p:P)->i32{if false{f(p)}else{0}}",
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        add(
            &mut cases,
            &format!("intermediate-{}{}{}", order[0], order[1], order[2]),
            format!(
                "type P={{x:i32}}\n{}\n{}\n{}\nfn main(){{f(record P{{x=42}})}}",
                functions[order[0]], functions[order[1]], functions[order[2]]
            ),
            "42",
        );
    }
    orders(
        &mut cases,
        "generic",
        "type R<a> ={value:a}",
        [
            "fn f(x){let r=g(x);let value=r.value;let alias=value;alias}",
            "fn g(x){if true{record R{value=x}}else{record R{value=f(x)}}}",
        ],
        "fn main(){let one=f(1);if f(true){42}else{0}}",
        "42",
    );
    orders(
        &mut cases,
        "unrelated-none",
        "type O<a> =None|Some(a) type R<a> ={value:a}",
        [
            "fn f(x){let r=g(x);let value=r.value;let none=None;let ints=if true{none}else{Some(1)};let bools=if true{none}else{Some(true)};value}",
            "fn g(x){if true{record R{value=x}}else{record R{value=f(x)}}}",
        ],
        "fn main(){if f(true){f(42)}else{0}}",
        "42",
    );
    // Only Pair's fresh second payload may quantify. Its first payload is the
    // weak projection alias, constrained to O<i32> by these two exact uses.
    orders(
        &mut cases,
        "selective-pair",
        "type O<a> =None|Some(a) type R<a> ={value:a} type Pair<a,b> =Pair(a,b)",
        [
            "fn f(x){let r=g(x);let value=r.value;let alias=value;let pair=Pair(alias,None);let ints=if true{pair}else{Pair(Some(1),Some(1))};let bools=if true{pair}else{Pair(Some(1),Some(true))};alias}",
            "fn g(x){if true{record R{value=x}}else{record R{value=f(x)}}}",
        ],
        "fn main(){match f(Some(42)){Some(n)=>n|None=>0}}",
        "42",
    );
    for (label, projection) in [
        ("nested-path", "r.inner.value"),
        ("nested-occurrences", "let inner=r.inner;inner.value"),
    ] {
        orders(
            &mut cases,
            label,
            "type Inner<a> ={value:a} type Outer<a> ={inner:Inner<a>}",
            [
                &format!("fn f(x){{let r=g(x);{projection}}}"),
                "fn g(x){if true{record Outer{inner=record Inner{value=x}}}else{record Outer{inner=record Inner{value=f(x)}}}}",
            ],
            "fn main(){if f(true){f(42)}else{0}}",
            "42",
        );
    }
    add(
        &mut cases,
        "reverse-dependency",
        "type Inner={value:i32} type Outer={inner:Inner}\nfn f(x,y){let value=y.value;let inner=x.inner;let equal=if true{y}else{inner};let known=if true{x}else{record Outer{inner=record Inner{value=0}}};value}\nfn main(){f(record Outer{inner=record Inner{value=42}},record Inner{value=42})}",
        "42",
    );
    add(
        &mut cases,
        "singleton-later-equality",
        "type R={value:i32}\nfn f(p){let value=p.value;let known=if true{p}else{record R{value=0}};value}\nfn main(){f(record R{value=42})}",
        "42",
    );
    add(
        &mut cases,
        "known-prefix-unknown-child",
        "type Inner={value:i32} type Outer<a> ={inner:a}\nfn f(p){let r=record Outer{inner=p};let value=r.inner.value;let known=if true{p}else{record Inner{value=0}};value}\nfn main(){f(record Inner{value=42})}",
        "42",
    );
    orders(
        &mut cases,
        "generic-substitution",
        "type O<a> =None|Some(a) type R<a,b,c> ={last:c;first:a;middle:b}",
        [
            "fn f(x,y){let r=g(x,y);r.middle}",
            "fn g(x,y){if true{record R{last=Some(x);first=y;middle=x}}else{record R{last=None;first=y;middle=f(x,y)}}}",
        ],
        "fn main(){if f(true,1){f(42,false)}else{0}}",
        "42",
    );
    for (label, functions, main, expected) in [
        (
            "read-root",
            "fn f(read p){let value=p.value;g(p);value} fn g(read p){let known=if true{p}else{record R{value=0}};if false{f(p)}else{0}}",
            "fn main(){f(record R{value=42})}",
            "42",
        ),
        (
            "inout-root",
            "fn f(inout p){p.value<-p.value+1;g(inout p);p.value} fn g(inout p){let known=if true{p}else{record R{value=0}};if false{f(inout p)}else{0}}",
            "fn main(){let mut p=record R{value=41};let result=f(inout p);result+p.value}",
            "84",
        ),
    ] {
        add(
            &mut cases,
            label,
            format!("type R={{value:i32}} {functions} {main}"),
            expected,
        );
    }
    add(
        &mut cases,
        "mutable-local",
        "type R<a> ={value:a}\nfn f(x){let mut p=g(x);p.value<-p.value+1;p.value}\nfn g(x){if true{record R{value=x}}else{record R{value=f(x)}}}\nfn main(){f(41)}",
        "42",
    );
    add(
        &mut cases,
        "pattern-root",
        "type R<a> ={value:a} type Wrap<a> =Wrap(a)\nfn f(x){let wrapped=g(x);match wrapped{Wrap(p)=>p.value}}\nfn g(x){if true{Wrap(record R{value=x})}else{Wrap(record R{value=f(x)})}}\nfn main(){if f(true){f(42)}else{0}}",
        "42",
    );
    for (order, schemas) in [
        "type I={a:i32;b:i32} type Outer={inner:I;other:i32}",
        "type I={b:i32;a:i32} type Outer={other:i32;inner:I}",
    ]
    .into_iter()
    .enumerate()
    {
        add(
            &mut cases,
            &format!("siblings-schema-order-{order}"),
            format!("{schemas}\nfn swap(inout a,inout b){{let old=a;a<-b;b<-old}}\nfn f(inout p){{swap(inout p.inner.a,inout p.inner.b);let known=if true{{p}}else{{record Outer{{other=0;inner=record I{{b=0;a=0}}}}}};p.inner.a*10+p.inner.b}}\nfn main(){{let mut p=record Outer{{inner=record I{{a=1;b=2}};other=0}};f(inout p)}}"),
            "21",
        );
    }
    let schemas = "type I={b:i32;a:i32} type Outer={other:i32;inner:I}";
    let value = "record Outer{other=0;inner=record I{b=2;a=1}}";
    let constrain = format!("let known=if true{{p}}else{{{value}}};");
    for (label, helpers, body, expected) in [
        (
            "shared-field-forwarding",
            "fn copy(read a,inout b){b<-a+41} fn forward(read a,inout b){copy(a,inout b)}",
            "forward(p.inner.a,inout p.inner.b);",
            "142",
        ),
        (
            "later-sibling-write",
            "fn set(inout a,n){a<-n}",
            "set(inout p.inner.a,{p.inner.b<-40;p.inner.b+2});",
            "4240",
        ),
        (
            "shadowed-root",
            "fn set(inout a,n){a<-n} fn bump(inout a){a<-a+1}",
            "set(inout p.inner.a,{let mut p=record Outer{other=0;inner=record I{b=0;a=41}};bump(inout p.inner.a);p.inner.a});",
            "4202",
        ),
        (
            "replace-prefix-original-slot",
            "",
            "p.inner.a<-{p.inner<-record I{b=30;a=7};42};",
            "4230",
        ),
        (
            "replace-root-original-slot",
            "",
            "p.inner.a<-{p<-record Outer{other=9;inner=record I{b=30;a=7}};42};",
            "4239",
        ),
    ] {
        add(
            &mut cases,
            label,
            format!("{schemas}\n{helpers}\nfn f(inout p){{{body}{constrain}p.inner.a*100+p.inner.b+p.other}}\nfn main(){{let mut p={value};f(inout p)}}"),
            expected,
        );
    }
    for (label, mode, argument) in [
        ("value-snapshot", "", "p.inner.a"),
        (
            "computed-read-snapshot",
            "read ",
            "{let snapshot=p.inner.a;snapshot}",
        ),
    ] {
        add(
            &mut cases,
            label,
            format!("{schemas}\nfn take({mode}before,inout a){{a<-a+1;before*100+a}}\nfn f(inout p){{let answer=take({argument},inout p.inner.a);{constrain}answer}}\nfn main(){{let mut p={value};f(inout p)}}"),
            "102",
        );
    }
    // Names must be compared by complete segments, not by text prefixes or by
    // concatenating a path: both `a` vs `ab` and `a.b` vs `ab` are siblings.
    add(
        &mut cases,
        "segment-sibling-identity",
        "type I={b:i32} type Outer={ab:i32;a:I}\nfn set(inout x,inout y){x<-20;y<-22}\nfn f(inout p){set(inout p.a.b,inout p.ab);let known=if true{p}else{record Outer{ab=0;a=record I{b=0}}};p.a.b+p.ab}\nfn main(){let mut p=record Outer{ab=0;a=record I{b=0}};f(inout p)}",
        "42",
    );
    add(
        &mut cases,
        "label-prefix-sibling-identity",
        "type R={ab:i32;a:i32}\nfn set(inout x,inout y){x<-20;y<-22}\nfn f(inout p){set(inout p.a,inout p.ab);let known=if true{p}else{record R{ab=0;a=0}};p.a+p.ab}\nfn main(){let mut p=record R{ab=0;a=0};f(inout p)}",
        "42",
    );
    add(
        &mut cases,
        "written-effects",
        "type R={a:i32;b:i32}\nfn f(read p){let answer=p.a*10+p.b;let known=if true{p}else{record R{a=0;b=0}};answer}\nfn main(){let mut n=0;let answer=f(record R{b={n<-n+1;n};a={n<-n+1;n}});answer*10+n}",
        "212",
    );
    // Each field read must instantiate the immutable root once, independently.
    // Simultaneous repeated and prefix shared paths remain legal across types.
    add(
        &mut cases,
        "polymorphic-nested-reads",
        "type O<a> =None|Some(a) type Inner<a> ={fixed:i32;value:O<a>} type Outer<a> ={inner:Inner<a>}\nfn ints(read x:O<i32>){match x{None=>40|Some(n)=>n}}\nfn bools(read x:O<bool>){match x{None=>2|Some(_)=>0}}\nfn take(read a:O<i32>,read b:O<bool>,read prefix:Inner<i32>){ints(a)+bools(b)+prefix.fixed}\nfn main(){let r=record Outer{inner=record Inner{fixed=0;value=None}};take(r.inner.value,r.inner.value,r.inner)}",
        "42",
    );
    for (label, body) in [
        (
            "deferred-read-division-trap",
            "let value=p.value;let known=if true{p}else{record R{value=0}};value/0",
        ),
        (
            "deferred-write-rhs-trap",
            "p.value<-{1/0};let known=if true{p}else{record R{value=0}};p.value",
        ),
    ] {
        cases.push(Case {
            label: label.into(),
            source: format!("type R={{value:i32}} fn f(inout p){{{body}}} fn main(){{let mut p=record R{{value=42}};f(inout p)}}"),
            expected: None,
            reference_c: true,
        });
    }
    // These back-edges execute: f's result projection is deferred until g's
    // body fixes the nominal record. Count main in the existing 32-call guard.
    for (label, main, expected) in [
        ("recursive-depth-31", "f(14)", Some("42")),
        ("recursive-depth-32", "let r=g(15);r.value", Some("42")),
        ("recursive-depth-33-trap", "f(15)", None),
    ] {
        cases.push(Case {
            label: label.into(),
            source: format!("type R={{value:i32}} fn f(n){{let r=g(n);r.value}} fn g(n){{if n==0{{record R{{value=42}}}}else{{record R{{value=f(n-1)}}}}}} fn main(){{{main}}}"),
            expected,
            reference_c: true,
        });
    }
    // The checked reference C layout oracle predates local-shape elaboration.
    // Keep these positive inference/typed cases, while asserting its existing
    // explicit rejection instead of widening that independent backend here.
    for case in &mut cases {
        if case.label.starts_with("unrelated-none-")
            || case.label.starts_with("selective-pair-")
            || case.label == "polymorphic-nested-reads"
        {
            case.reference_c = false;
        }
    }
    cases
}
