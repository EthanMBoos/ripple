use quote::quote;
use std::{env, fs, path::PathBuf};
use syn::{ImplItem, Item};
struct Annotate;
impl syn::visit_mut::VisitMut for Annotate {
    fn visit_expr_for_loop_mut(&mut self, expr: &mut syn::ExprForLoop) {
        expr.attrs
            .push(syn::parse_quote!(#[invariant(conversion_prefix(produced.inner(), received@))]));
        syn::visit_mut::visit_expr_for_loop_mut(self, expr);
    }
    fn visit_expr_match_mut(&mut self, expr: &mut syn::ExprMatch) {
        if let syn::Expr::MethodCall(call) = &*expr.expr
            && call.method == "downcast"
        {
            let body = &expr.arms[1].body;
            *expr.arms[1].body = syn::parse_quote!({
                proof_assert! {
                    let i = received@.len();
                    0 <= i && i < _original_queue.inner().len() &&
                    cast::<T, std::alloc::Global, _>(_original_queue.inner()[i].value) == None
                };
                #body
            });
        }
    }
}
fn main() {
    println!("cargo::rustc-check-cfg=cfg(creusot)");
    println!("cargo:rerun-if-env-changed=RIPPLE_VERIFY_SOURCE");
    let source = env::var_os("RIPPLE_VERIFY_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../crates/core/src/pubsub/messages.rs"));
    println!("cargo:rerun-if-changed={}", source.display());
    let file = syn::parse_file(&fs::read_to_string(source).unwrap()).unwrap();
    let names = [
        "MessageEndpoint",
        "Message",
        "Publication",
        "Envelope",
        "Messages",
    ];
    let aliases = ["Channel", "Payload", "PayloadSerializer"];
    let mut output = quote! { use anyhow::Result; use serde_json::Value;
    use std::{any::{Any, TypeId}, collections::BTreeMap, sync::Arc};
    use creusot_std::prelude::*; };
    let mut receive_count = 0;
    for item in file.items {
        match item {
            Item::Struct(mut item) if names.iter().any(|name| item.ident == name) => {
                item.attrs.clear();
                output.extend(quote! { #item });
            }
            Item::Type(item) if aliases.iter().any(|name| item.ident == name) => {
                output.extend(quote! { #item });
            }
            Item::Fn(mut item)
                if ["subscription_error", "payload_type_error"]
                    .iter()
                    .any(|n| item.sig.ident == n) =>
            {
                item.attrs.push(syn::parse_quote!(#[trusted]));
                output.extend(quote! { #item });
            }
            Item::Impl(item) => {
                for member in item.items {
                    if let ImplItem::Fn(mut method) = member
                        && method.sig.ident == "receive"
                    {
                        receive_count += 1;
                        method.attrs.extend([
                            syn::parse_quote!(#[ensures((^self).subscriptions == self.subscriptions)]),
                            syn::parse_quote!(#[ensures((^self).outbox == self.outbox)]),
                            syn::parse_quote!(#[ensures((^self).time_s == self.time_s)]),
                            syn::parse_quote!(#[ensures(
                                !subscribed::<T>(*self, network@, topic@) ==> ^self == *self
                            )]),
                            syn::parse_quote!(#[ensures(
                                subscribed::<T>(*self, network@, topic@) ==>
                                (^self).inbox@ == self.inbox@.remove((network@, topic@))
                            )]),
                            syn::parse_quote!(#[ensures(match result {
                                Ok(values) =>
                                    subscribed::<T>(*self, network@, topic@) &&
                                    conversion_prefix(queue(*self, network@, topic@), values@),
                                Err(_) =>
                                    !subscribed::<T>(*self, network@, topic@) ||
                                    exists<i> 0 <= i &&
                                        i < queue(*self, network@, topic@).len() &&
                                        cast::<T, std::alloc::Global, _>(
                                            queue(*self, network@, topic@)[i].value
                                        ) == None,
                            })]),
                        ]);
                        let index = method
                            .block
                            .stmts
                            .iter()
                            .position(|stmt| match stmt {
                                syn::Stmt::Local(local) => {
                                    matches!(&local.pat, syn::Pat::Ident(p) if p.ident == "queued")
                                }
                                _ => false,
                            })
                            .unwrap();
                        method.block.stmts.insert(
                            index + 1,
                            syn::parse_quote!(let _original_queue = snapshot!(queued@);),
                        );
                        syn::visit_mut::VisitMut::visit_block_mut(&mut Annotate, &mut method.block);
                        output.extend(quote! { impl Messages { #method } });
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        receive_count, 1,
        "expected exactly one production receive method"
    );
    let target = PathBuf::from(env::var("OUT_DIR").unwrap()).join("messages.rs");
    fs::write(&target, output.to_string()).unwrap();
    assert!(
        std::process::Command::new("rustfmt")
            .args(["--edition", "2024"])
            .arg(&target)
            .status()
            .unwrap()
            .success()
    );
    fs::create_dir_all("target").unwrap();
    fs::copy(&target, "target/receive.generated.rs").unwrap();
}
