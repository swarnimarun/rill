//! The builtin table the resolver and the checker share.

/// Every name the resolver accepts as a builtin must have a signature the
/// checker can type a call with, or a call reports an error no user can fix.
/// `fail` and `zeroed` are the two the checker types by its own path checks
/// instead: completion offers them, and shows them without a signature.
#[test]
fn every_builtin_name_has_a_signature() {
    let (mut frontend, _sm) =
        rill_ast::parse_and_lower_text("builtins.rill", "let main () -> i32 <computes> = 0;\n");
    let hir = rill_hir::lower(&mut frontend);
    let checked = rill_ty::check(&frontend, &hir);
    for name in rill_hir::BUILTIN_NAMES {
        let signature = checked.builtin_sig(name);
        if matches!(*name, "fail" | "zeroed") {
            assert!(
                signature.is_none(),
                "`{name}` is typed by a path check, not by the table"
            );
        } else {
            assert!(
                signature.is_some(),
                "`{name}` is a builtin the checker cannot type a call with"
            );
        }
    }
}
