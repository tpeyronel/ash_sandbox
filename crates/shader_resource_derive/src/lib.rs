use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{parse_macro_input, spanned::Spanned, DeriveInput};

#[proc_macro_derive(ShaderStruct)]
pub fn derive_shader_struct_declaration_provider(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
        let input = parse_macro_input!(input as DeriveInput);

        let name = &input.ident;
        let declaration = shader_struct_declaration(&input);

        let implementation = quote! {
                impl crate::shader_resource::ShaderStruct for #name {
                        fn shader_struct_declaration() -> crate::shader_resource::ShaderStructDeclaration {
                                #declaration
                        }
                }
        };

        let output = proc_macro::TokenStream::from(implementation);

        // println!("{}", output);

        output
}

fn shader_struct_declaration(input: &DeriveInput) -> TokenStream {
        let type_name = &input.ident.to_string();
        let fields = shader_struct_declaration_fields(input);

        quote! {
                crate::shader_resource::ShaderStructDeclaration {
                        type_name: #type_name.to_string(),
                        fields: vec![
                                #fields
                        ],
                }
        }
}

fn shader_struct_declaration_fields(input: &DeriveInput) -> TokenStream {
        let data = &input.data;

        match data {
                syn::Data::Enum(_) | syn::Data::Union(_) => syn::Error::new_spanned(
                        &input.ident,
                        "#[derive(ShaderStruct)] must only be used with named structs",
                )
                .to_compile_error(),
                syn::Data::Struct(data) => match &data.fields {
                        syn::Fields::Unit => syn::Error::new_spanned(
                                &input.ident,
                                "#[derive(ShaderStruct)] must not be used with unit structs",
                        )
                        .to_compile_error(),
                        syn::Fields::Unnamed(_) => syn::Error::new_spanned(
                                &input.ident,
                                "#[derive(ShaderStruct)] must not be used with unnamed structs",
                        )
                        .to_compile_error(),
                        syn::Fields::Named(fields) => {
                                let children = fields.named.iter().map(|f| shader_struct_field(f));

                                quote! {
                                        #(#children),*
                                }
                        },
                },
        }
}

fn shader_struct_field(field: &syn::Field) -> TokenStream {
        let field_name = field.ident.as_ref().unwrap().to_string();
        let ty = &field.ty;

        quote_spanned! {field.span()=>
                crate::shader_resource::ShaderStructField {
                        field_name: #field_name.to_string(),
                        field_type: <#ty as crate::shader_resource::ShaderStructFieldTypeProvider>::shader_struct_field_type(),
                }
        }
}
