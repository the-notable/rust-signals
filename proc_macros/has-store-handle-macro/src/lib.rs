use proc_macro::TokenStream;

use quote::{quote, ToTokens};
use syn::ItemStruct;

#[proc_macro_attribute]
pub fn has_store_handle(_args: TokenStream, input: TokenStream) -> TokenStream {
    
    let ast: ItemStruct = syn::parse(input).unwrap();

    let attrs = &ast.attrs;
    let vis = &ast.vis.to_token_stream();
    let name = &ast.ident.to_token_stream();
    let (
        impl_generics, 
        ty_generics, 
        where_clause
    ) = &ast.generics.split_for_impl();
    
    let fields = &ast.fields
        .iter()
        .map(|v| v.to_token_stream())
        .collect::<Vec<_>>();

    // println!("{}", name);
    // println!("{}", impl_generics.to_token_stream());
    // println!("{}", ty_generics.to_token_stream());
    // println!("{}", where_clause.to_token_stream());
    // fields.iter().for_each(|v| println!("{}", v));

    // Build the output, possibly using quasi-quotation
    let expanded = quote! {
        #(#attrs)*
        #vis struct #name #impl_generics #where_clause {
            #(#fields),*,
            store_handle: StoreHandle
        }
        
        impl #impl_generics HasStoreHandle for #name #ty_generics #where_clause {
            fn store_handle(&self) -> &StoreHandle {
                &self.store_handle
            }
        }
    };

    // Hand the output tokens back to the compiler
    TokenStream::from(expanded)
}