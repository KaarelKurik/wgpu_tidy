use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, parse_macro_input, punctuated::Punctuated, token::Comma};

#[proc_macro_derive(Writable)]
pub fn derive_writable(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Extract struct name
    let name = &input.ident;

    // Prepare implementation based on struct fields
    let write_implementation = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields) => {
                let field_writes = fields
                    .named
                    .iter()
                    .enumerate()
                    .map(|(index, field)| {
                        let field_name = &field.ident;
                        let field_ty = &field.ty;
                        let index_u32 = index as u32;

                        quote! {
                            let cursor = c.navigate_field(#index_u32).unwrap();
                            self.#field_name.write_at_cursor(cursor, device, queue, binding_resources)
                        }
                    })
                    .collect::<Vec<_>>();

                quote! {
                    fn write_at_cursor(
                        &self,
                        c: reflection::Cursor,
                        device: &wgpu::Device,
                        queue: &wgpu::Queue,
                        binding_resources: &mut reflection::BindingResources,
                    ) {
                        #(#field_writes;)*
                    }
                }
            }
            Fields::Unnamed(fields) => {
                let field_writes = fields
                    .unnamed
                    .iter()
                    .enumerate()
                    .map(|(index, field)| {
                        let field_ty = &field.ty;
                        let field_ident = format_ident!("_{}", index);
                        let index_u32 = index as u32;

                        quote! {
                            let cursor = c.navigate_field(#index_u32).unwrap();
                            self.#field_ident.write_at_cursor(cursor, device, queue, binding_resources)
                        }
                    })
                    .collect::<Vec<_>>();

                quote! {
                    fn write_at_cursor(
                        &self,
                        c: reflection::Cursor,
                        device: &wgpu::Device,
                        queue: &wgpu::Queue,
                        binding_resources: &mut reflection::BindingResources,
                    ) {
                        #(#field_writes;)*
                    }
                }
            }
            Fields::Unit => {
                quote! {
                    fn write_at_cursor(
                        &self,
                        c: reflection::Cursor,
                        device: &wgpu::Device,
                        queue: &wgpu::Queue,
                        binding_resources: &mut reflection::BindingResources,
                    ) {
                    }
                }
            }
        },
        _ => panic!("Writable can only be derived for structs"),
    };

    let expanded = quote! {
        impl Writable for #name {
            #write_implementation
        }
    };

    proc_macro::TokenStream::from(expanded)
}
