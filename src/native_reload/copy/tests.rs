use super::*;
use crate::{native_reload::Optimization, parser};
use std::{fmt::Write, process::Command};

fn candidate(delta: i32, optimization: Optimization) -> CopyCandidate {
    let program = parser::parse(&format!(r#"
type Mode = Quiet | Boost(i32)
type State = {{ ticks: i32; weight: f32; mode: Mode }}
fn update(state: State) -> State {{
    record State {{ ticks = state.ticks + {delta}; weight = f32_add(state.weight, f32(2)); mode = state.mode }}
}}
fn observe(state: State) -> i32 {{
    match state.mode {{ Quiet => state.ticks | Boost(n) => state.ticks + n }}
}}
"#)).unwrap();
    CopyCandidate::build(
        &program,
        &[
            Export {
                source: "update".into(),
                c_name: "rill_update".into(),
            },
            Export {
                source: "observe".into(),
                c_name: "rill_observe".into(),
            },
        ],
        &NativeCompiler::new("cc", optimization),
    )
    .unwrap()
}
fn state(ticks: i32, weight: f32) -> Value {
    Value::Record(vec![
        ("ticks".into(), Value::I32(ticks)),
        ("weight".into(), Value::F32(weight.to_bits())),
        (
            "mode".into(),
            Value::Enum {
                variant: "Boost".into(),
                fields: vec![Value::I32(4)],
            },
        ),
    ])
}
fn retained(session: &mut CopySession) {
    assert_eq!(session.generation(), 0);
    let next = session.call("rill_update", &[state(3, 0.5)]).unwrap();
    assert_eq!(next, state(4, 2.5));
    assert_eq!(
        session.call("rill_observe", &[next]).unwrap(),
        Value::I32(8)
    );
}
fn rewrite(candidate: &CopyCandidate, c: &str) {
    let source = candidate._directory.0.join("fault.c");
    fs::write(&source, c).unwrap();
    let result = Command::new("cc")
        .args([
            "-std=c11",
            "-fPIC",
            if cfg!(target_os = "macos") {
                "-dynamiclib"
            } else {
                "-shared"
            },
        ])
        .arg(source)
        .arg("-o")
        .arg(&candidate.library)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn copy_records_enums_and_floats_survive_repeated_native_replacement() {
    for optimization in [Optimization::O0, Optimization::O3] {
        let first = candidate(1, optimization);
        let mut previous = first._directory.0.clone();
        let mut session = CopySession::new(first).unwrap();
        retained(&mut session);
        for generation in 1..=3 {
            let next = candidate(4, optimization);
            let path = next._directory.0.clone();
            assert_ne!(previous, path);
            session.replace(next).unwrap();
            assert_eq!(session.generation(), generation);
            assert!(!previous.exists());
            assert!(path.exists());
            assert_eq!(
                session.call("rill_update", &[state(3, -0.0)]).unwrap(),
                state(7, 2.0)
            );
            previous = path;
        }
        drop(session);
        assert!(!previous.exists());
    }
}

#[test]
fn admission_and_contract_failures_preserve_code_and_cleanup_candidates() {
    let mut session = CopySession::new(candidate(1, Optimization::O0)).unwrap();
    assert!(session.call("missing", &[]).is_err());
    assert!(session.call("rill_update", &[Value::I32(3)]).is_err());
    assert!(session.call("rill_update", &[]).is_err());
    let changed = parser::parse("fn observe(x: bool) -> bool { x }").unwrap();
    let changed = CopyCandidate::build(
        &changed,
        &[Export {
            source: "observe".into(),
            c_name: "rill_observe".into(),
        }],
        &NativeCompiler::default(),
    )
    .unwrap();
    let path = changed._directory.0.clone();
    assert!(session
        .replace(changed)
        .unwrap_err()
        .contains("contracts changed"));
    assert!(!path.exists());
    retained(&mut session);
    session.generation = u64::MAX;
    let next = candidate(5, Optimization::O0);
    let path = next._directory.0.clone();
    assert!(session.replace(next).unwrap_err().contains("exhausted"));
    assert!(!path.exists());
    assert_eq!(
        session.call("rill_observe", &[state(3, 0.5)]).unwrap(),
        Value::I32(7)
    );
}

#[test]
fn bad_images_wrong_identities_and_missing_dispatchers_do_not_publish() {
    let mut session = CopySession::new(candidate(1, Optimization::O0)).unwrap();
    let invalid = candidate(2, Optimization::O0);
    fs::write(&invalid.library, b"invalid image").unwrap();
    assert!(session
        .replace(invalid)
        .unwrap_err()
        .contains("cannot load"));
    retained(&mut session);
    let stale = candidate(2, Optimization::O0);
    fs::copy(&session.active.candidate.library, &stale.library).unwrap();
    assert!(session
        .replace(stale)
        .unwrap_err()
        .contains("identity mismatch"));
    retained(&mut session);
    let missing = candidate(2, Optimization::O0);
    rewrite(&missing, "int unrelated(void) { return 0; }\n");
    assert!(session
        .replace(missing)
        .unwrap_err()
        .contains("missing versioned bridge"));
    retained(&mut session);
    let missing = candidate(2, Optimization::O0);
    let mut c = String::from("#include <stdint.h>\nstatic const unsigned char bytes[] = {");
    for byte in &missing.identity {
        write!(c, "{byte},").unwrap();
    }
    c.push_str("};\nuint32_t rill_abi_copy_identity_v1(unsigned char *out, uint32_t cap) { uint32_t n = sizeof(bytes); if (out && cap >= n) for(uint32_t i=0;i<n;i++) out[i]=bytes[i]; return n; }\n");
    rewrite(&missing, &c);
    assert!(session
        .replace(missing)
        .unwrap_err()
        .contains("missing versioned bridge"));
    retained(&mut session);
}
