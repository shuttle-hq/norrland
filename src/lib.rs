use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    token::{Comma, Pub, Trait},
    FnArg, Ident, ImplItem, ItemImpl, ItemTrait, Pat, Path, Signature, TraitItem, TraitItemFn,
    Type, TypePath, Visibility,
};

#[proc_macro_attribute]
pub fn norrland(args: TokenStream, tokens: TokenStream) -> TokenStream {
    let db_type = parse_macro_input!(args as TypePath);
    let input = parse_macro_input!(tokens as ItemImpl);

    // Extract the struct name
    let name = match &input.self_ty.as_ref() {
        Type::Path(path) => path.path.segments.last().unwrap().ident.clone(),
        _ => return quote! { compile_error!("Expected an impl block for a named struct"); }.into(),
    };

    // Extract methods
    let fns = input
        .items
        .iter()
        .filter_map(|item| match item {
            ImplItem::Fn(method) => Some(method),
            _ => None,
        })
        .collect::<Vec<_>>();

    // Generate trait with just the function definitions
    let trait_fns = fns
        .iter()
        .map(|func| {
            TraitItem::Fn(TraitItemFn {
                // Empty vec instead of `func.attrs.clone()` to ignore applying attributes in trait definition.
                // This prevents things like `#[tracing::instrument]` from breaking it.
                // TODO: might break other scenarios.
                attrs: Vec::new(),
                sig: remove_mut_bindings(func.sig.clone()),
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
        trait_token: Trait(name.span()),
        ident: name.clone(),
        generics: input.generics.clone(),
        colon_token: None,
        supertraits: Punctuated::new(),
        restriction: None,
        brace_token: Default::default(),
        items: trait_fns,
    };

    let pool_impl_fns = fns
        .clone()
        .into_iter()
        .map(|func| {
            let mut modified_func = func.clone();
            modified_func.sig = remove_mut_bindings(func.sig.clone());

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

    // Generate trait implementation
    let impl_trait_block = quote! {
        type _Dummy = #db_type;

        #trait_def

        impl #name for &mut #conn {
            #(#fns)*
        }

        impl #name for &#pool {
            #(#pool_impl_fns)*
        }
    };

    impl_trait_block.into()
}

fn remove_mut_bindings(mut sig: Signature) -> Signature {
    sig.inputs.iter_mut().for_each(|i| {
        if let FnArg::Typed(typed) = i {
            if let Pat::Ident(ident) = typed.pat.as_mut() {
                // Remove any (owned) mut binding of arg
                ident.mutability = None;
                // Remove subpatterns
                ident.subpat = None;
            }
        }
    });

    sig
}
