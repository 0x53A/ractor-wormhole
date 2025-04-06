use crate::util::bail;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use venial::NamedField;

// Usage
// -----------------------------------------------------------------------------------------------------------------------------------

// This macro can be applied to structs and enums, and will derive ractor_wormhole::transmaterialization::ContextTransmaterializable.

// (for AI)
// As a reminder, the trait looks like this:
/*
#[async_trait]
pub trait ContextTransmaterializable {
    async fn immaterialize(self, ctx: &TransmaterializationContext) -> TransmaterializationResult<Vec<u8>>;
    async fn rematerialize(ctx: &TransmaterializationContext, data: &[u8]) -> TransmaterializationResult<Self>
    where
        Self: Sized;
}
*/

// -----------------------------------------------------------------------------------------------------------------------------------
// <begin of example>

#[cfg(false)]
pub struct Dummy {
    pub dummy: u32,
}

#[cfg(false)]
#[::ractor_wormhole::transmaterialization::transmaterialization_proxies::async_trait]
impl ::ractor_wormhole::transmaterialization::ContextTransmaterializable for Dummy {
    async fn immaterialize(
        self,
        ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext,
    ) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Vec<u8>> {
        let mut buffer = Vec::new();

        let field_bytes_dummy =
            <u32 as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::immaterialize(
                self.dummy, ctx,
            )
            .await?;
        buffer.extend_from_slice(&(field_bytes_dummy.len() as u64).to_le_bytes());
        buffer.extend_from_slice(&field_bytes_dummy);

        Ok(buffer)
    }

    async fn rematerialize(
        ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext,
        data: &[u8],
    ) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Self> {
        let mut offset = 0;

        let field_len_dummy = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
        offset += 8;
        let field_bytes_dummy = &data[offset..offset + field_len_dummy];
        offset += field_len_dummy;
        let field_dummy =
            <u32 as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::rematerialize(
                ctx,
                field_bytes_dummy,
            )
            .await?;

        if data.len() != offset {
            return Err(
                ::ractor_wormhole::transmaterialization::transmaterialization_proxies::anyhow!(
                    "Rematerialization did not consume all data! Buffer length: {}, consumed: {}",
                    data.len(),
                    offset
                ),
            );
        }

        Ok(Self { dummy: field_dummy })
    }
}

// <end of example>
// -----------------------------------------------------------------------------------------------------------------------------------

// -----------------------------------------------------------------------------------------------------------------------------------
// <begin of example>

#[cfg(false)]
pub enum DummyEnum {
    Case1(u32, String),
    Case2,
    Case3 { field1: u32, field2: String },
}

#[cfg(false)]
#[::ractor_wormhole::transmaterialization::transmaterialization_proxies::async_trait]
impl ::ractor_wormhole::transmaterialization::ContextTransmaterializable for DummyEnum {
    async fn immaterialize(
        self,
        ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext,
    ) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Vec<u8>> {
        let mut buffer = Vec::new();

        let (case, bytes): (String, Vec<u8>) = match self {
            DummyEnum::Case1(field1, field2) => {
                let field_bytes_field1 =
                    <u32 as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::immaterialize(
                        field1, ctx,
                    )
                    .await?;
                let field_bytes_field2 =
                    <String as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::immaterialize(
                        field2, ctx,
                    )
                    .await?;
                let mut buffer = Vec::new();
                buffer.extend_from_slice(&(field_bytes_field1.len() as u64).to_le_bytes());
                buffer.extend_from_slice(&field_bytes_field1);
                buffer.extend_from_slice(&(field_bytes_field2.len() as u64).to_le_bytes());
                buffer.extend_from_slice(&field_bytes_field2);
                ("Case1".to_string(), buffer)
            }
            DummyEnum::Case2 => ("Case2".to_string(), vec![]),
            DummyEnum::Case3 { field1, field2 } => {
                let field_bytes_field1 =
                    <u32 as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::immaterialize(
                        field1, ctx,
                    )
                    .await?;
                let field_bytes_field2 =
                    <String as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::immaterialize(
                        field2, ctx,
                    )
                    .await?;
                let mut buffer = Vec::new();
                buffer.extend_from_slice(&(field_bytes_field1.len() as u64).to_le_bytes());
                buffer.extend_from_slice(&field_bytes_field1);
                buffer.extend_from_slice(&(field_bytes_field2.len() as u64).to_le_bytes());
                buffer.extend_from_slice(&field_bytes_field2);
                ("Case3".to_string(), buffer)
            }
        };
        buffer.extend_from_slice(&(case.len() as u64).to_le_bytes());
        buffer.extend_from_slice(case.as_bytes());
        buffer.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        buffer.extend_from_slice(&bytes);

        Ok(buffer)
    }

    async fn rematerialize(
        ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext,
        data: &[u8],
    ) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Self> {
        let mut offset = 0;

        // Read the variant name
        let variant_name_len = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
        offset += 8;
        let variant_name_bytes = &data[offset..offset + variant_name_len];
        offset += variant_name_len;
        let variant_name = std::str::from_utf8(variant_name_bytes)?.to_string();

        // Read the payload data length
        let payload_data_len = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
        offset += 8;
        let payload_data = &data[offset..offset + payload_data_len];
        offset += payload_data_len;

        // Construct the enum variant based on the name
        let result = match variant_name.as_str() {
            "Case1" => {
                let mut payload_offset = 0;

                // Read the first field (u32)
                let field1_len = u64::from_le_bytes(
                    payload_data[payload_offset..payload_offset + 8].try_into()?,
                ) as usize;
                payload_offset += 8;
                let field1_bytes = &payload_data[payload_offset..payload_offset + field1_len];
                payload_offset += field1_len;
                let field1 =
                    <u32 as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::rematerialize(
                        ctx,
                        field1_bytes,
                    )
                    .await?;

                // Read the second field (String)
                let field2_len = u64::from_le_bytes(
                    payload_data[payload_offset..payload_offset + 8].try_into()?,
                ) as usize;
                payload_offset += 8;
                let field2_bytes = &payload_data[payload_offset..payload_offset + field2_len];
                payload_offset += field2_len;
                let field2 =
                    <String as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::rematerialize(
                        ctx,
                        field2_bytes,
                    )
                    .await?;

                Self::Case1(field1, field2)
            }
            "Case2" => Self::Case2,
            "Case3" => {
                let mut payload_offset = 0;

                // Read field1 (u32)
                let field1_len = u64::from_le_bytes(
                    payload_data[payload_offset..payload_offset + 8].try_into()?,
                ) as usize;
                payload_offset += 8;
                let field1_bytes = &payload_data[payload_offset..payload_offset + field1_len];
                payload_offset += field1_len;
                let field1 =
                    <u32 as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::rematerialize(
                        ctx,
                        field1_bytes,
                    )
                    .await?;

                // Read field2 (String)
                let field2_len = u64::from_le_bytes(
                    payload_data[payload_offset..payload_offset + 8].try_into()?,
                ) as usize;
                payload_offset += 8;
                let field2_bytes = &payload_data[payload_offset..payload_offset + field2_len];
                payload_offset += field2_len;
                let field2 =
                    <String as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::rematerialize(
                        ctx,
                        field2_bytes,
                    )
                    .await?;

                Self::Case3 { field1, field2 }
            }
            _ => {
                return Err(
                    ::ractor_wormhole::transmaterialization::transmaterialization_proxies::anyhow!(
                        "Unknown variant: {}",
                        variant_name
                    ),
                );
            }
        };

        if data.len() != offset {
            return Err(
                ::ractor_wormhole::transmaterialization::transmaterialization_proxies::anyhow!(
                    "Rematerialization did not consume all data! Buffer length: {}, consumed: {}",
                    data.len(),
                    offset
                ),
            );
        }

        Ok(result)
    }
}

// <end of example>
// -----------------------------------------------------------------------------------------------------------------------------------

fn for_field(field: &NamedField) -> Result<(TokenStream, TokenStream), venial::Error> {
    let field_name = field.name.clone();
    let field_type = field.ty.clone();

    let ident_field_bytes = format_ident!("field_bytes_{field_name}");
    let ident_field_len = format_ident!("field_len_{field_name}");
    let ident_field = format_ident!("field_{field_name}");

    let has_attr_serde = field.attributes.iter().any(|a| {
        matches!(
            a.get_single_path_segment()
                .map(|s| s.to_string())
                .as_deref(),
            Some("serde")
        )
    });
    let has_attr_bincode = field.attributes.iter().any(|a| {
        matches!(
            a.get_single_path_segment()
                .map(|s| s.to_string())
                .as_deref(),
            Some("bincode")
        )
    });

    if has_attr_serde && has_attr_bincode {
        return bail!(
            field,
            "Field {field_name} cannot have both #[serde] and #[bincode] attributes."
        );
    }
    if has_attr_serde {
        return bail!(field, "#[serde] attribute is not yet implemented");
    }
    if has_attr_bincode {
        return bail!(field, "#[bincode] attribute is not yet implemented");
    }

    let serialize = quote! {
        let #ident_field_bytes = <#field_type as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::immaterialize(self.#field_name, ctx).await?;
        buffer.extend_from_slice(&(#ident_field_bytes.len() as u64).to_le_bytes());
        buffer.extend_from_slice(&#ident_field_bytes);
    };

    let deserialize = quote! {
        let #ident_field_len = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
        offset += 8;
        let #ident_field_bytes = &data[offset..offset + #ident_field_len];
        offset += #ident_field_len;
        let #ident_field = <#field_type as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::rematerialize(ctx, #ident_field_bytes).await?;
    };

    Ok((serialize, deserialize))
}

fn derive_struct(input: venial::Struct) -> Result<proc_macro2::TokenStream, venial::Error> {
    let struct_name = input.name.clone();

    // Extract generic parameters
    let generic_params = input.generic_params.clone();

    // Generate impl generics and type generics
    let impl_generics = if let Some(generic_params) = &generic_params {
        quote! { #generic_params }
    } else {
        quote! {}
    };

    // Generate the type with its generics for the impl block
    let type_generics = if let Some(generic_params) = &generic_params {
        let params = generic_params.params.iter().map(|(param, _)| {
            let ident = &param.name;
            quote! { #ident }
        });
        quote! { <#(#params),*> }
    } else {
        quote! {}
    };

    // Generate trait bounds for generic parameters
    let extended_where_clause = input.create_derive_where_clause(
        quote! {::ractor_wormhole::transmaterialization::ContextTransmaterializable},
    );

    match &input.fields {
        venial::Fields::Named(named_fields) => {
            let fields = named_fields
                .fields
                .iter()
                .map(|(field, _)| for_field(field))
                .collect::<Result<Vec<_>, _>>()?;

            let (serialize, deserialize): (Vec<_>, Vec<_>) = fields.into_iter().unzip();

            // Create the struct reconstruction with all fields
            let field_names = named_fields.fields.iter().map(|(field, _)| {
                let field_name = field.name.clone();
                let ident_field = format_ident!("field_{field_name}");
                quote! { #field_name: #ident_field }
            });

            let q = quote! {
                #[::ractor_wormhole::transmaterialization::transmaterialization_proxies::async_trait]
                impl #impl_generics ::ractor_wormhole::transmaterialization::ContextTransmaterializable for #struct_name #type_generics #extended_where_clause {
                    async fn immaterialize(self, ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Vec<u8> >  {
                        let mut buffer = Vec::new();

                        #(#serialize)*

                        Ok(buffer)
                    }

                    async fn rematerialize(ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext, data: &[u8]) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Self>  {
                        let mut offset = 0;

                        #(#deserialize)*

                        if data.len() != offset {
                            return Err(::ractor_wormhole::transmaterialization::transmaterialization_proxies::anyhow!("Rematerialization did not consume all data! Buffer length: {}, consumed: {}", data.len(), offset));
                        }

                        Ok(Self { #(#field_names),* })
                    }
                }
            };

            Ok(q)
        }
        venial::Fields::Tuple(tuple_fields) => {
            // Handle tuple structs like `struct UserAlias(String)`
            let field_count = tuple_fields.fields.len();
            let field_types: Vec<_> = tuple_fields
                .fields
                .iter()
                .map(|(f, _)| f.ty.clone())
                .collect();

            // Generate serialization code for tuple fields
            let mut serialize_fields = Vec::new();
            for i in 0..field_count {
                let field_type = &field_types[i];
                let field_bytes_ident = format_ident!("field_bytes_{}", i);
                let index = proc_macro2::Literal::usize_unsuffixed(i);

                let serialize_field = quote! {
                    let #field_bytes_ident = <#field_type as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::immaterialize(
                        self.#index, ctx
                    ).await?;
                    buffer.extend_from_slice(&(#field_bytes_ident.len() as u64).to_le_bytes());
                    buffer.extend_from_slice(&#field_bytes_ident);
                };
                serialize_fields.push(serialize_field);
            }

            // Generate deserialization code for tuple fields
            let mut deserialize_fields = Vec::new();
            let mut field_value_idents = Vec::new();
            for i in 0..field_count {
                let field_type = &field_types[i];
                let field_len_ident = format_ident!("field{}_len", i);
                let field_bytes_ident = format_ident!("field{}_bytes", i);
                let field_value_ident = format_ident!("field{}_value", i);
                field_value_idents.push(field_value_ident.clone());

                deserialize_fields.push(quote! {
                    let #field_len_ident = u64::from_le_bytes(
                        data[offset..offset + 8].try_into()?
                    ) as usize;
                    offset += 8;
                    let #field_bytes_ident = &data[offset..offset + #field_len_ident];
                    offset += #field_len_ident;
                    let #field_value_ident =
                        <#field_type as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::rematerialize(
                            ctx, #field_bytes_ident
                        ).await?;
                });
            }

            let q = quote! {
                #[::ractor_wormhole::transmaterialization::transmaterialization_proxies::async_trait]
                impl #impl_generics ::ractor_wormhole::transmaterialization::ContextTransmaterializable for #struct_name #type_generics #extended_where_clause {
                    async fn immaterialize(self, ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Vec<u8> >  {
                        let mut buffer = Vec::new();

                        #(#serialize_fields)*

                        Ok(buffer)
                    }

                    async fn rematerialize(ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext, data: &[u8]) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Self>  {
                        let mut offset = 0;

                        #(#deserialize_fields)*

                        if data.len() != offset {
                            return Err(::ractor_wormhole::transmaterialization::transmaterialization_proxies::anyhow!("Rematerialization did not consume all data! Buffer length: {}, consumed: {}", data.len(), offset));
                        }

                        Ok(Self(#(#field_value_idents),*))
                    }
                }
            };

            Ok(q)
        }
        venial::Fields::Unit => {
            // Handle unit structs like `struct EmptyStruct;`
            let q = quote! {
                #[::ractor_wormhole::transmaterialization::transmaterialization_proxies::async_trait]
                impl #impl_generics ::ractor_wormhole::transmaterialization::ContextTransmaterializable for #struct_name #type_generics #extended_where_clause {
                    async fn immaterialize(self, _ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Vec<u8> >  {
                        // Unit structs have no data to immaterialize
                        Ok(Vec::new())
                    }

                    async fn rematerialize(_ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext, data: &[u8]) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Self>  {
                        // Ensure we received empty data
                        if data.len() != 0 {
                            return Err(::ractor_wormhole::transmaterialization::transmaterialization_proxies::anyhow!("Unit struct should have no data to rematerialize, but buffer has len={}", data.len()));
                        }
                        Ok(Self)
                    }
                }
            };

            Ok(q)
        }
    }
}

fn derive_enum(input: venial::Enum) -> Result<proc_macro2::TokenStream, venial::Error> {
    let enum_name = input.name.clone();

    // Generate match arms for serialization
    let mut serialize_arms = Vec::new();

    // Generate match arms for deserialization
    let mut deserialize_arms = Vec::new();

    for (variant, _) in input.variants.iter() {
        let variant_name = &variant.name;
        let variant_name_str = variant_name.to_string();

        match &variant.fields {
            // Unit variant (e.g., Case2)
            venial::Fields::Unit => {
                serialize_arms.push(quote! {
                    #enum_name::#variant_name => (#variant_name_str.to_string(), vec![]),
                });

                deserialize_arms.push(quote! {
                    #variant_name_str => Self::#variant_name,
                });
            }

            // Tuple variant (e.g., Case1(u32, String))
            venial::Fields::Tuple(fields) => {
                let field_count = fields.fields.len();
                let field_idents: Vec<_> = (0..field_count)
                    .map(|i| format_ident!("field{}", i))
                    .collect();
                let field_types: Vec<_> = fields.fields.iter().map(|(f, _)| f.clone()).collect();

                // Serialization for tuple variant fields
                let mut serialize_fields = Vec::new();
                for (i, field_type) in field_types.iter().enumerate() {
                    let field_ident = &field_idents[i];
                    let field_bytes_ident = format_ident!("field_bytes_{}", i);

                    let serialize_field = quote! {
                        let #field_bytes_ident = <#field_type as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::immaterialize(
                            #field_ident, ctx
                        ).await?;
                        buffer.extend_from_slice(&(#field_bytes_ident.len() as u64).to_le_bytes());
                        buffer.extend_from_slice(&#field_bytes_ident);
                    };

                    serialize_fields.push(serialize_field);
                }

                serialize_arms.push(quote! {
                    #enum_name::#variant_name(#(#field_idents),*) => {
                        let mut buffer = Vec::new();
                        #(#serialize_fields)*
                        (#variant_name_str.to_string(), buffer)
                    },
                });

                // Deserialization for tuple variant fields
                let mut deserialize_fields = Vec::new();
                let mut field_value_idents = Vec::new();

                for (i, field_type) in field_types.iter().enumerate() {
                    let field_len_ident = format_ident!("field{}_len", i);
                    let field_bytes_ident = format_ident!("field{}_bytes", i);
                    let field_value_ident = format_ident!("field{}_value", i);
                    field_value_idents.push(field_value_ident.clone());

                    deserialize_fields.push(quote! {
                        let #field_len_ident = u64::from_le_bytes(
                            payload_data[payload_offset..payload_offset + 8].try_into()?
                        ) as usize;
                        payload_offset += 8;
                        let #field_bytes_ident = &payload_data[payload_offset..payload_offset + #field_len_ident];
                        payload_offset += #field_len_ident;
                        let #field_value_ident =
                            <#field_type as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::rematerialize(
                                ctx, #field_bytes_ident
                            ).await?;
                    });
                }

                deserialize_arms.push(quote! {
                    #variant_name_str => {
                        let mut payload_offset = 0;
                        #(#deserialize_fields)*
                        Self::#variant_name(#(#field_value_idents),*)
                    },
                });
            }

            // Struct variant (e.g., Case3 { field1: u32, field2: String })
            venial::Fields::Named(named_fields) => {
                let field_defs = named_fields
                    .fields
                    .iter()
                    .map(|(field, _)| for_field(field))
                    .collect::<Result<Vec<_>, _>>()?;

                let field_names: Vec<_> = named_fields
                    .fields
                    .iter()
                    .map(|(field, _)| field.name.clone())
                    .collect();

                // Generate field serialization
                let (serialize_field_codes, _): (Vec<_>, Vec<_>) = field_defs.into_iter().unzip();

                serialize_arms.push(quote! {
                    #enum_name::#variant_name { #(#field_names),* } => {
                        let mut buffer = Vec::new();
                        #(#serialize_field_codes)*
                        (#variant_name_str.to_string(), buffer)
                    },
                });

                // Generate field deserialization
                let mut deserialize_fields = Vec::new();
                let mut field_value_pairs = Vec::new();

                for field in named_fields.fields.iter() {
                    let field_name = field.0.name.clone();
                    let field_type = field.0.ty.clone();
                    let field_len_ident = format_ident!("field_len_{}", field_name);
                    let field_bytes_ident = format_ident!("field_bytes_{}", field_name);
                    let field_value_ident = format_ident!("field_{}", field_name);

                    deserialize_fields.push(quote! {
                        let #field_len_ident = u64::from_le_bytes(
                            payload_data[payload_offset..payload_offset + 8].try_into()?
                        ) as usize;
                        payload_offset += 8;
                        let #field_bytes_ident = &payload_data[payload_offset..payload_offset + #field_len_ident];
                        payload_offset += #field_len_ident;
                        let #field_value_ident =
                            <#field_type as ::ractor_wormhole::transmaterialization::ContextTransmaterializable>::rematerialize(
                                ctx, #field_bytes_ident
                            ).await?;
                    });

                    field_value_pairs.push(quote! {
                        #field_name: #field_value_ident
                    });
                }

                deserialize_arms.push(quote! {
                    #variant_name_str => {
                        let mut payload_offset = 0;
                        #(#deserialize_fields)*
                        Self::#variant_name { #(#field_value_pairs),* }
                    },
                });
            }
        }
    }

    // Complete implementation
    let q = quote! {
        #[::ractor_wormhole::transmaterialization::transmaterialization_proxies::async_trait]
        impl ::ractor_wormhole::transmaterialization::ContextTransmaterializable for #enum_name {
            async fn immaterialize(
                self,
                ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext,
            ) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Vec<u8>> {
                let mut buffer = Vec::new();

                let (case, bytes): (String, Vec<u8>) = match self {
                    #(#serialize_arms)*
                };
                buffer.extend_from_slice(&(case.len() as u64).to_le_bytes());
                buffer.extend_from_slice(case.as_bytes());
                buffer.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
                buffer.extend_from_slice(&bytes);

                Ok(buffer)
            }

            async fn rematerialize(
                ctx: &::ractor_wormhole::transmaterialization::TransmaterializationContext,
                data: &[u8],
            ) -> ::ractor_wormhole::transmaterialization::TransmaterializationResult<Self> {
                let mut offset = 0;

                // Read the variant name
                let variant_name_len = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
                offset += 8;
                let variant_name_bytes = &data[offset..offset + variant_name_len];
                offset += variant_name_len;
                let variant_name = std::str::from_utf8(variant_name_bytes)?.to_string();

                // Read the payload data length
                let payload_data_len = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
                offset += 8;
                let payload_data = &data[offset..offset + payload_data_len];
                offset += payload_data_len;

                // Construct the enum variant based on the name
                let result = match variant_name.as_str() {
                    #(#deserialize_arms)*
                    _ => {
                        return Err(::ractor_wormhole::transmaterialization::transmaterialization_proxies::anyhow!("Unknown variant: {}", variant_name));
                    }
                };

                if data.len() != offset {
                    return Err(::ractor_wormhole::transmaterialization::transmaterialization_proxies::anyhow!("Rematerialization did not consume all data! Buffer length: {}, consumed: {}", data.len(), offset));
                }

                Ok(result)
            }
        }
    };

    Ok(q)
}

pub fn derive_wormhole_serializable_impl(
    input: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream, venial::Error> {
    let input_decl = venial::parse_item(input)?;

    match input_decl {
        venial::Item::Struct(struct_decl) => derive_struct(struct_decl),
        venial::Item::Enum(enum_decl) => derive_enum(enum_decl),
        _ => bail!(
            input_decl,
            "WormholeSerializable can only be derived for structs and enums."
        ),
    }
}
