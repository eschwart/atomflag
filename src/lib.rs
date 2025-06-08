use proc_macro::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{DeriveInput, Expr, Ident, Lit, Meta, parse_macro_input};

fn impl_opt_ptr(name: &Ident, new_name: impl ToTokens) -> impl ToTokens {
    quote! {
        impl std::ops::BitAnd<#name> for #new_name {
            type Output = #name;

            fn bitand(self, rhs: #name) -> Self::Output {
                let bits = self.0.fetch_and(rhs.bits(), std::sync::atomic::Ordering::Relaxed);
                #name::from_bits_retain(bits)
            }
        }

        impl std::ops::BitAndAssign<#name> for #new_name {
            fn bitand_assign(&mut self, rhs: #name) {
                self.0.fetch_and(rhs.bits(), std::sync::atomic::Ordering::Relaxed);
            }
        }

        impl std::ops::BitOr<#name> for #new_name {
            type Output = #name;

            fn bitor(self, rhs: #name) -> Self::Output {
                let bits = self.0.fetch_or(rhs.bits(), std::sync::atomic::Ordering::Relaxed);
                #name::from_bits_retain(bits)
            }
        }

        impl std::ops::BitOrAssign<#name> for #new_name {
            fn bitor_assign(&mut self, rhs: #name) {
                self.0.fetch_or(rhs.bits(), std::sync::atomic::Ordering::Relaxed);
            }
        }

        impl std::ops::Sub<#name> for #new_name {
            type Output = #name;

            fn sub(self, rhs: #name) -> Self::Output {
                let bits = self.0.fetch_and(!rhs.bits(), std::sync::atomic::Ordering::Relaxed);
                #name::from_bits_retain(bits)
            }
        }

        impl std::ops::SubAssign<#name> for #new_name {
            fn sub_assign(&mut self, rhs: #name) {
                self.0.fetch_and(!rhs.bits(), std::sync::atomic::Ordering::Relaxed);
            }
        }

        impl PartialEq<#name> for #new_name {
            fn eq(&self, other: &#name) -> bool {
                self.get() == *other
            }
        }

        impl From<#new_name> for #name {
            fn from(value: #new_name) -> Self {
                value.get()
            }
        }
    }
}

#[proc_macro_derive(AtomicFlag, attributes(atomic_flag))]
pub fn atomflag_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    // parse the optional #[atomic_flag(wrapper = "...")]
    let mut wrapper_type = None;
    for attr in &input.attrs {
        if attr.path().is_ident("atomic_flag") {
            let meta = attr.parse_args::<Meta>().unwrap();

            if let Meta::NameValue(nv) = meta {
                if nv.path.is_ident("wrapper") {
                    match &nv.value {
                        Expr::Lit(expr) => match &expr.lit {
                            Lit::Str(s) => wrapper_type = Some(s.value()),
                            _ => panic!("Expected string literal."),
                        },
                        _ => panic!("Expected literal expression."),
                    }
                }
            }
        }
    }

    let new_name = Ident::new(&format!("Atomic{}", name), Span::call_site().into());

    let mut derives = vec![];

    let inner_atomic =
        quote! { <<#name as ::bitflags::Flags>::Bits as ::atomint::AtomicInt>::Atomic };
    let wrapped_atomic = match wrapper_type.as_deref() {
        Some("Arc") => {
            derives.push(quote! { Clone });
            quote! { std::sync::Arc<#inner_atomic> }
        }
        Some("Rc") => {
            derives.push(quote! { Clone });
            quote! { std::rc::Rc<#inner_atomic> }
        }
        _ => inner_atomic.clone(), // default: no wrapper
    };
    derives.push(quote! { Default });

    let new_impl = match wrapper_type.as_deref() {
        Some("Arc") => quote! {
            pub fn new(value: <#name as ::bitflags::Flags>::Bits) -> Self {
                Self(std::sync::Arc::new(<#inner_atomic>::new(value)))
            }
        },
        Some("Rc") => quote! {
            pub fn new(value: <#name as ::bitflags::Flags>::Bits) -> Self {
                Self(std::rc::Rc::new(<#inner_atomic>::new(value)))
            }
        },
        Some(_) => panic!("Only 'Arc' and 'Rc' are supported."),
        None => quote! {
            pub const fn new(value: <#name as ::bitflags::Flags>::Bits) -> Self {
                Self(<#inner_atomic>::new(value))
            }
        },
    };

    let struct_impl = quote! {
        #[repr(transparent)]
        #[derive(#(#derives),*)]
        pub struct #new_name(#wrapped_atomic);

        impl #new_name {
            #new_impl

            pub fn bits(&self) -> <#name as ::bitflags::Flags>::Bits {
                self.0.load(std::sync::atomic::Ordering::Relaxed)
            }

            pub fn get(&self) -> #name {
                #name::from_bits_retain(self.bits())
            }

            pub fn is_empty(&self) -> bool {
                self.get().is_empty()
            }

            pub fn contains(&self, other: #name) -> bool {
                self.get().contains(other)
            }

            pub fn clear(&self) {
                self.0.store(0, std::sync::atomic::Ordering::Relaxed);
            }
        }

        impl std::fmt::Display for #new_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&format!("{:?}", self.get()))
            }
        }

        impl std::fmt::Debug for #new_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self, f)
            }
        }
    };

    let default_impl = impl_opt_ptr(name, quote! { #new_name });
    let borrow_impl = impl_opt_ptr(name, quote! { &#new_name });
    let borrow_mut_impl = impl_opt_ptr(name, quote! { &mut #new_name });

    quote! {
        #struct_impl
        #default_impl
        #borrow_impl
        #borrow_mut_impl
    }
    .into()
}
