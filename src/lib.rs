#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    token::{And, Comma, Pub, Trait},
    FnArg, Ident, ImplItem, ItemImpl, ItemTrait, Pat, Path, Signature, TraitItem, TraitItemFn,
    Type, TypePath, TypeReference, Visibility,
};

#[proc_macro_attribute]
pub fn norrland(args: TokenStream, tokens: TokenStream) -> TokenStream {
    let db_type = parse_macro_input!(args as TypePath);
    let input = parse_macro_input!(tokens as ItemImpl);

    // Extract the struct name
    let trait_name = match input.trait_ {
        Some(trt) => trt.1.segments.last().unwrap().ident.clone(),
        None => return quote! { compile_error!("Expected an impl block for a trait on a named struct"); }.into(),
    };
    let struct_name = match &input.self_ty.as_ref() {
        Type::Path(path) => path.path.segments.last().unwrap().ident.clone(),
        _ => return quote! { compile_error!("Expected an impl block for a trait on a named struct"); }.into(),
    };

    // Extract non-private methods for main trait
    let fns = input
        .items
        .iter()
        .filter_map(|item| match item {
            ImplItem::Fn(method) => match method.vis {
                Visibility::Inherited => None,
                _ => Some(method),
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    // TODO: extract private fns and place them in a private inner trait

    // Generate trait with just the function definitions
    let trait_fns = fns
        .iter()
        .map(|func| {
            let mut sig = func.sig.clone();
            remove_mut_bindings(&mut sig);

            TraitItem::Fn(TraitItemFn {
                // Empty vec instead of `func.attrs.clone()` to ignore applying attributes in trait definition.
                // This prevents things like `#[tracing::instrument]` from breaking it.
                // TODO: might break other scenarios.
                attrs: Vec::new(),
                sig,
                default: None,
                semi_token: Some(Default::default()),
            })
        })
        .collect::<Vec<_>>();
    let trait_def = ItemTrait {
        attrs: vec![],
        vis: Visibility::Public(Pub::default()),
        unsafety: None,
        auto_token: None,
        trait_token: Trait(trait_name.span()),
        ident: trait_name.clone(),
        generics: input.generics.clone(),
        colon_token: None,
        supertraits: Punctuated::new(),
        restriction: None,
        brace_token: Default::default(),
        items: trait_fns,
    };

    let conn_impl_fns = fns
        .iter()
        .map(|&func| {
            let mut modified_func = func.clone();
            modified_func.vis = Visibility::Inherited; // remove `pub`, etc

            modified_func
        })
        .collect::<Vec<_>>();

    let pool_impl_fns = fns
        .iter()
        .map(|&func| {
            let mut modified_func = func.clone();
            remove_mut_bindings(&mut modified_func.sig);
            modified_func.vis = Visibility::Inherited; // remove `pub`, etc

            let ident = func.sig.ident.clone();
            let args_to_conn = func
                .sig
                .inputs
                .clone()
                .into_iter()
                .filter_map(|a| match a {
                    FnArg::Typed(typed) => Some(typed),
                    _ => None,
                })
                .filter_map(|pt| match *pt.pat {
                    Pat::Ident(ident) => Some(ident.ident),
                    _ => None,
                })
                .collect::<Punctuated<Ident, Comma>>();
            modified_func.block.stmts = parse_quote! {
                let mut conn = self.acquire().await?;
                conn.#ident(#args_to_conn).await
            };

            modified_func
        })
        .collect::<Vec<_>>();

    let struct_impl_fns = fns
        .iter()
        .map(|&func| {
            let mut modified_func = func.clone();
            remove_mut_bindings(&mut modified_func.sig);
            // change `self` to `&self`
            if let Some(first_arg) = modified_func.sig.inputs.first_mut() {
                if let FnArg::Receiver(receiver) = first_arg {
                    receiver.reference = Some((And::default(), None));
                    receiver.ty = Box::new(Type::Reference(TypeReference {
                        and_token: And::default(),
                        lifetime: None,
                        mutability: None,
                        elem: receiver.ty.clone(),
                    }))
                }
            }
            // (don't remove `pub`, etc)

            let ident = func.sig.ident.clone();
            let args_to_conn = func
                .sig
                .inputs
                .clone()
                .into_iter()
                .filter_map(|a| match a {
                    FnArg::Typed(typed) => Some(typed),
                    _ => None,
                })
                .filter_map(|pt| match *pt.pat {
                    Pat::Ident(ident) => Some(ident.ident),
                    _ => None,
                })
                .collect::<Punctuated<Ident, Comma>>();
            modified_func.block.stmts = parse_quote! {
                self.pool.#ident(#args_to_conn).await
            };

            modified_func
        })
        .collect::<Vec<_>>();

    let (conn, pool): (Path, Path) = match db_type
        .path
        .segments
        .last()
        .unwrap()
        .ident
        .to_string()
        .as_str()
    {
        "Postgres" => (
            parse_quote!(::sqlx::PgConnection),
            parse_quote!(::sqlx::PgPool),
        ),
        "MySql" => (
            parse_quote!(::sqlx::MySqlConnection),
            parse_quote!(::sqlx::MySqlPool),
        ),
        _ => return quote! { compile_error!("Expected an sqlx Database type"); }.into(),
    };

    // Create trait definitions and implementations
    let impl_trait_block = quote! {
        type _Dummy = #db_type;

        #trait_def

        impl #trait_name for &mut #conn {
            #(#conn_impl_fns)*
        }

        impl #trait_name for &#pool {
            #(#pool_impl_fns)*
        }

        #[derive(Clone, Debug)]
        pub struct #struct_name {
            pub pool: #pool,
        }
        impl #struct_name {
            pub fn new(pool: #pool) -> Self {
                Self { pool }
            }
        }

        impl #struct_name {
            #(#struct_impl_fns)*
        }
    };

    impl_trait_block.into()
}

fn remove_mut_bindings(sig: &mut Signature) {
    sig.inputs.iter_mut().for_each(|arg| {
        if let FnArg::Typed(typed) = arg {
            if let Pat::Ident(ident) = typed.pat.as_mut() {
                // Remove any (owned) mut binding of arg
                ident.mutability = None;
                // Remove subpatterns
                ident.subpat = None;
            }
        }
    });
}
