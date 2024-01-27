use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{parse_macro_input, spanned::Spanned, DeriveInput};

#[proc_macro_derive(ShaderStruct)]
pub fn derive_shader_struct_declaration_provider(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
        let input = parse_macro_input!(input as DeriveInput);

        assert_repr_c(&input);

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

fn assert_repr_c(input: &DeriveInput) {
        let mut repr_c = false;
        for attr in &input.attrs {
                if attr.path().is_ident("repr") {
                        let _ = attr.parse_nested_meta(|meta| {
                                // #[repr(C)]
                                if meta.path.is_ident("C") {
                                        repr_c = true;
                                }

                                Ok(())
                        });
                }
        }

        assert!(repr_c, "struct is not marked with #[repr(C)]");
}

fn shader_struct_declaration(input: &DeriveInput) -> TokenStream {
        let type_name = &input.ident.to_string();
        let fields = shader_struct_declaration_fields(&input.data);

        quote! {
                crate::shader_resource::ShaderStructDeclaration {
                        type_name: #type_name.to_string(),
                        fields: vec![
                                #fields
                        ],
                }
        }
}

fn shader_struct_declaration_fields(data: &syn::Data) -> TokenStream {
        match data {
                syn::Data::Enum(_) | syn::Data::Union(_) => {
                        panic!("ShaderStruct must only be derived for structs")
                },
                syn::Data::Struct(data) => match &data.fields {
                        syn::Fields::Unnamed(_) => panic!("fields must be named"),
                        syn::Fields::Unit => panic!("must contain at least one field"),
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
                ShaderStructField {
                        field_name: #field_name.to_string(),
                        field_type: <#ty as crate::shader_resource::ShaderStructFieldTypeProvider>::shader_struct_field_type(),
                }
        }
}
