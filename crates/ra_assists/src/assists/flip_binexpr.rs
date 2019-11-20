use hir::db::HirDatabase;
use ra_syntax::ast::{AstNode, BinExpr, BinOp};

use crate::{Assist, AssistCtx, AssistId};

// Assist: flip_binexpr
//
// Flips operands of a binary expression.
//
// ```
// fn main() {
//     let _ = 90 +<|> 2;
// }
// ```
// ->
// ```
// fn main() {
//     let _ = 2 + 90;
// }
// ```
pub(crate) fn flip_binexpr(ctx: AssistCtx<impl HirDatabase>) -> Option<Assist> {
    let expr = ctx.find_node_at_offset::<BinExpr>()?;
    let lhs = expr.lhs()?.syntax().clone();
    let rhs = expr.rhs()?.syntax().clone();
    let op_range = expr.op_token()?.text_range();
    // The assist should be applied only if the cursor is on the operator
    let cursor_in_range = ctx.frange.range.is_subrange(&op_range);
    if !cursor_in_range {
        return None;
    }
    let action: FlipAction = expr.op_kind()?.into();
    // The assist should not be applied for certain operators
    if let FlipAction::DontFlip = action {
        return None;
    }

    ctx.add_assist(AssistId("flip_binexpr"), "flip binary expression", |edit| {
        edit.target(op_range);
        if let FlipAction::FlipAndReplaceOp(new_op) = action {
            edit.replace(op_range, new_op);
        }
        edit.replace(lhs.text_range(), rhs.text());
        edit.replace(rhs.text_range(), lhs.text());
    })
}

enum FlipAction {
    // Flip the expression
    Flip,
    // Flip the expression and replace the operator with this string
    FlipAndReplaceOp(&'static str),
    // Do not flip the expression
    DontFlip,
}

impl From<BinOp> for FlipAction {
    fn from(op_kind: BinOp) -> Self {
        match op_kind {
            BinOp::Assignment => FlipAction::DontFlip,
            BinOp::AddAssign => FlipAction::DontFlip,
            BinOp::DivAssign => FlipAction::DontFlip,
            BinOp::MulAssign => FlipAction::DontFlip,
            BinOp::RemAssign => FlipAction::DontFlip,
            BinOp::ShrAssign => FlipAction::DontFlip,
            BinOp::ShlAssign => FlipAction::DontFlip,
            BinOp::SubAssign => FlipAction::DontFlip,
            BinOp::BitOrAssign => FlipAction::DontFlip,
            BinOp::BitAndAssign => FlipAction::DontFlip,
            BinOp::BitXorAssign => FlipAction::DontFlip,
            BinOp::GreaterTest => FlipAction::FlipAndReplaceOp("<"),
            BinOp::GreaterEqualTest => FlipAction::FlipAndReplaceOp("<="),
            BinOp::LesserTest => FlipAction::FlipAndReplaceOp(">"),
            BinOp::LesserEqualTest => FlipAction::FlipAndReplaceOp(">="),
            _ => FlipAction::Flip,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::helpers::{check_assist, check_assist_not_applicable, check_assist_target};

    #[test]
    fn flip_binexpr_target_is_the_op() {
        check_assist_target(flip_binexpr, "fn f() { let res = 1 ==<|> 2; }", "==")
    }

    #[test]
    fn flip_binexpr_not_applicable_for_assignment() {
        check_assist_not_applicable(flip_binexpr, "fn f() { let mut _x = 1; _x +=<|> 2 }")
    }

    #[test]
    fn flip_binexpr_works_for_eq() {
        check_assist(
            flip_binexpr,
            "fn f() { let res = 1 ==<|> 2; }",
            "fn f() { let res = 2 ==<|> 1; }",
        )
    }

    #[test]
    fn flip_binexpr_works_for_gt() {
        check_assist(
            flip_binexpr,
            "fn f() { let res = 1 ><|> 2; }",
            "fn f() { let res = 2 <<|> 1; }",
        )
    }

    #[test]
    fn flip_binexpr_works_for_lteq() {
        check_assist(
            flip_binexpr,
            "fn f() { let res = 1 <=<|> 2; }",
            "fn f() { let res = 2 >=<|> 1; }",
        )
    }

    #[test]
    fn flip_binexpr_works_for_complex_expr() {
        check_assist(
            flip_binexpr,
            "fn f() { let res = (1 + 1) ==<|> (2 + 2); }",
            "fn f() { let res = (2 + 2) ==<|> (1 + 1); }",
        )
    }

    #[test]
    fn flip_binexpr_works_inside_match() {
        check_assist(
            flip_binexpr,
            r#"
            fn dyn_eq(&self, other: &dyn Diagnostic) -> bool {
                match other.downcast_ref::<Self>() {
                    None => false,
                    Some(it) => it ==<|> self,
                }
            }
            "#,
            r#"
            fn dyn_eq(&self, other: &dyn Diagnostic) -> bool {
                match other.downcast_ref::<Self>() {
                    None => false,
                    Some(it) => self ==<|> it,
                }
            }
            "#,
        )
    }
}
