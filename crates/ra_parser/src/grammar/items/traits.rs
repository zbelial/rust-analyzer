//! FIXME: write short doc here

use super::*;

// test trait_item
// trait T<U>: Hash + Clone where U: Copy {}
// trait X<U: Debug + Display>: Hash + Clone where U: Copy {}
pub(super) fn trait_def(p: &mut Parser) {
    assert!(p.at(T![trait]));
    p.bump(T![trait]);
    name_r(p, ITEM_RECOVERY_SET);
    type_params::opt_type_param_list(p);
    if p.at(T![:]) {
        type_params::bounds(p);
    }
    type_params::opt_where_clause(p);
    if p.at(T!['{']) {
        trait_item_list(p);
    } else {
        p.error("expected `{`");
    }
}

// test trait_item_list
// impl F {
//     type A: Clone;
//     const B: i32;
//     fn foo() {}
//     fn bar(&self);
// }
pub(crate) fn trait_item_list(p: &mut Parser) {
    assert!(p.at(T!['{']));
    let m = p.start();
    p.bump(T!['{']);
    while !p.at(EOF) && !p.at(T!['}']) {
        if p.at(T!['{']) {
            error_block(p, "expected an item");
            continue;
        }
        item_or_macro(p, true, ItemFlavor::Trait);
    }
    p.expect(T!['}']);
    m.complete(p, ITEM_LIST);
}

// test impl_block
// impl Foo {}
pub(super) fn impl_block(p: &mut Parser) {
    assert!(p.at(T![impl]));
    p.bump(T![impl]);
    if choose_type_params_over_qpath(p) {
        type_params::opt_type_param_list(p);
    }

    // FIXME: never type
    // impl ! {}

    // test impl_block_neg
    // impl !Send for X {}
    p.eat(T![!]);
    impl_type(p);
    if p.eat(T![for]) {
        impl_type(p);
    }
    type_params::opt_where_clause(p);
    if p.at(T!['{']) {
        impl_item_list(p);
    } else {
        p.error("expected `{`");
    }
}

// test impl_item_list
// impl F {
//     type A = i32;
//     const B: i32 = 92;
//     fn foo() {}
//     fn bar(&self) {}
// }
pub(crate) fn impl_item_list(p: &mut Parser) {
    assert!(p.at(T!['{']));
    let m = p.start();
    p.bump(T!['{']);
    // test impl_inner_attributes
    // enum F{}
    // impl F {
    //      //! This is a doc comment
    //      #![doc("This is also a doc comment")]
    // }
    attributes::inner_attributes(p);

    while !p.at(EOF) && !p.at(T!['}']) {
        if p.at(T!['{']) {
            error_block(p, "expected an item");
            continue;
        }
        item_or_macro(p, true, ItemFlavor::Mod);
    }
    p.expect(T!['}']);
    m.complete(p, ITEM_LIST);
}

fn choose_type_params_over_qpath(p: &Parser) -> bool {
    // There's an ambiguity between generic parameters and qualified paths in impls.
    // If we see `<` it may start both, so we have to inspect some following tokens.
    // The following combinations can only start generics,
    // but not qualified paths (with one exception):
    //     `<` `>` - empty generic parameters
    //     `<` `#` - generic parameters with attributes
    //     `<` (LIFETIME|IDENT) `>` - single generic parameter
    //     `<` (LIFETIME|IDENT) `,` - first generic parameter in a list
    //     `<` (LIFETIME|IDENT) `:` - generic parameter with bounds
    //     `<` (LIFETIME|IDENT) `=` - generic parameter with a default
    // The only truly ambiguous case is
    //     `<` IDENT `>` `::` IDENT ...
    // we disambiguate it in favor of generics (`impl<T> ::absolute::Path<T> { ... }`)
    // because this is what almost always expected in practice, qualified paths in impls
    // (`impl <Type>::AssocTy { ... }`) aren't even allowed by type checker at the moment.
    if !p.at(T![<]) {
        return false;
    }
    if p.nth(1) == T![#] || p.nth(1) == T![>] {
        return true;
    }
    (p.nth(1) == LIFETIME || p.nth(1) == IDENT)
        && (p.nth(2) == T![>] || p.nth(2) == T![,] || p.nth(2) == T![:] || p.nth(2) == T![=])
}

// test_err impl_type
// impl Type {}
// impl Trait1 for T {}
// impl impl NotType {}
// impl Trait2 for impl NotType {}
pub(crate) fn impl_type(p: &mut Parser) {
    if p.at(T![impl]) {
        p.error("expected trait or type");
        return;
    }
    types::type_(p);
}
