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
fn annotate_receive(method: &mut syn::ImplItemFn) {
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
}

fn main() {
    println!("cargo::rustc-check-cfg=cfg(creusot)");
    println!("cargo:rerun-if-env-changed=RIPPLE_VERIFY_SOURCE");
    let source = env::var_os("RIPPLE_VERIFY_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../../crates/core/src/pubsub/messages.rs"));
    println!("cargo:rerun-if-changed={}", source.display());
    let file = syn::parse_file(&fs::read_to_string(source).unwrap()).unwrap();
    if env::var_os("CARGO_FEATURE_ROUTING").is_some() {
        write_output(routing_module(file));
        return;
    }
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
                        annotate_receive(&mut method);
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
    write_output(output);
}

fn write_output(output: proc_macro2::TokenStream) {
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

fn routing_module(mut file: syn::File) -> proc_macro2::TokenStream {
    use syn::visit_mut::VisitMut;
    struct IndexLoops {
        subscriber: bool,
    }
    impl VisitMut for IndexLoops {
        fn visit_expr_for_loop_mut(&mut self, expr: &mut syn::ExprForLoop) {
            expr.attrs.push(if self.subscriber {
                syn::parse_quote!(#[invariant(valid_subscribers(subscribers, mailboxes@, within_entity))])
            } else {
                syn::parse_quote!(#[invariant(valid_receivers(receivers, mailboxes@))])
            });
            if !self.subscriber {
                expr.body.stmts.insert(0, syn::parse_quote!(proof_assert! {
                    index@ == produced.inner().len() - 1 && mailboxes@[index@].endpoint.deep_model() == mailbox.endpoint.deep_model()
                };));
                expr.body.stmts.insert(0, syn::parse_quote!(proof_assert! {
                    creusot_std::std::iter::IteratorSpec::produces(iter_old.inner(), produced.inner(), iter)
                };));
                expr.body.stmts.insert(
                    1,
                    syn::parse_quote!(proof_assert! {
                    index@ == produced.inner().len() - 1 && index@ < mailboxes@.len() &&
                    produced.inner()[index@] == (index, mailbox)
                };),
                );
                expr.body.stmts.insert(
                    3,
                    syn::parse_quote!(proof_assert! {
                    mailbox_endpoints(mailboxes@)[index@] == mailbox.endpoint.deep_model() &&
                    index@ < mailbox_endpoints(mailboxes@).len()
                };),
                );
                expr.body.stmts.insert(4, syn::parse_quote!(proof_assert! {
                    mailbox_endpoints(mailboxes@).subsequence(0, produced.inner().len()) ==
                        mailbox_endpoints(mailboxes@).subsequence(0, index@).push_back(mailbox.endpoint.deep_model())
                };));
                expr.body.stmts.push(syn::parse_quote!(proof_assert! {
                    receivers@.ext_eq(endpoint_index(mailbox_endpoints(mailboxes@).subsequence(0, produced.inner().len())))
                };));
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    receivers@ == endpoint_index(mailbox_endpoints(mailboxes@).subsequence(0, produced.inner().len()))
                )]));
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    forall<i> 0 <= i && i < produced.inner().len() ==>
                        receivers@.get(mailboxes@[i].endpoint.deep_model()) != None
                )]));
            }
            if self.subscriber
                && quote!(#expr)
                    .to_string()
                    .contains("for (channel , expected_type)")
                && !quote!(#expr).to_string().contains("for (index , mailbox)")
            {
                expr.body.stmts.push(syn::parse_quote!(proof_assert! {
                    match subscribers@.get((entity.deep_model(), channel.deep_model())) {
                        None => false,
                        Some(entries) => matching_entries((entity.deep_model(), channel.deep_model()), entries@, mailboxes@, within_entity),
                    }
                };));
                expr.body.stmts.insert(0, syn::parse_quote!(proof_assert! {
                    match subscribers@.get((entity.deep_model(), channel.deep_model())) {
                        None => true,
                        Some(entries) => matching_entries((entity.deep_model(), channel.deep_model()), entries@, mailboxes@, within_entity),
                    }
                };));
                expr.body.stmts.insert(
                    0,
                    syn::parse_quote!(proof_assert! {
                    subscriber_matches((entity.deep_model(), channel.deep_model()),
                        (index, *expected_type), mailboxes@, within_entity)
                };),
                );
                expr.body.stmts.insert(
                    0,
                    syn::parse_quote!(proof_assert! {
                    produced.inner()[produced.inner().len() - 1] == (channel, expected_type)
                };),
                );
                expr.body.stmts.insert(
                    0,
                    syn::parse_quote!(proof_assert! {
                    mailboxes@[index@].endpoint.deep_model() == mailbox.endpoint.deep_model()
                };),
                );
            }
            syn::visit_mut::visit_expr_for_loop_mut(self, expr);
        }
    }
    #[derive(Default)]
    struct RoutingLoops {
        links: usize,
        deliveries: usize,
        appends: usize,
        schedules: usize,
    }
    impl VisitMut for RoutingLoops {
        fn visit_expr_for_loop_mut(&mut self, expr: &mut syn::ExprForLoop) {
            let collection = &expr.expr;
            let text = quote!(#collection).to_string();
            expr.body.stmts.insert(0, syn::parse_quote!(proof_assert! {
                creusot_std::std::iter::IteratorSpec::produces(iter_old.inner(), produced.inner(), iter)
            };));
            if text.contains("mailboxes . iter_mut") {
                expr.body.stmts.insert(
                    1,
                    syn::parse_quote!(proof_assert! {
                    produced.inner().len() <= _route_outboxes.inner().len() &&
                    mailbox.messages.outbox@ == _route_outboxes.inner()[produced.inner().len() - 1]
                };),
                );
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    produced.inner().len() + (*iter@)@.len() == _route_outboxes.inner().len() &&
                    (forall<i> 0 <= i && i < (*iter@)@.len() ==>
                        (*iter@)@[i].messages.outbox@ == _route_outboxes.inner()[i + produced.inner().len()])
                )]));
                expr.body.stmts.insert(2, syn::parse_quote!(proof_assert! {
                    mailbox.messages.inbox@ == _route_queues.inner()[produced.inner().len() - 1] &&
                    mailbox.endpoint.deep_model() == _route_endpoints.inner()[produced.inner().len() - 1] &&
                    (_quiet.inner() ==> forall<j> 0 <= j && j < mailbox.messages.outbox@.len() ==>
                        mailbox.messages.outbox@[j].channel.0@ != _network_name.inner())
                };));
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    forall<i> 0 <= i && i < produced.inner().len() ==>
                        (^produced.inner()[i]).messages.inbox@ == _route_queues.inner()[i] &&
                        (^produced.inner()[i]).endpoint.deep_model() == _route_endpoints.inner()[i]
                )]));
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    forall<i> 0 <= i && i < (*iter@)@.len() ==>
                        (*iter@)@[i].messages.inbox@ == _route_queues.inner()[i + produced.inner().len()] &&
                        (*iter@)@[i].endpoint.deep_model() == _route_endpoints.inner()[i + produced.inner().len()]
                )]));
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    _quiet.inner() ==> forall<i, j> 0 <= i && i < (*iter@)@.len() &&
                        0 <= j && j < (*iter@)@[i].messages.outbox@.len() ==>
                        (*iter@)@[i].messages.outbox@[j].channel.0@ != _network_name.inner()
                )]));
            }
            if text.contains("messages . outbox") {
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    _publication_source.inner() == produced.inner().concat(iter@)
                )]));
                expr.body.stmts.insert(
                    1,
                    syn::parse_quote!(proof_assert! {
                    _quiet.inner() ==> publication.channel.0@ != _network_name.inner()
                };),
                );
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    _quiet.inner() ==> forall<i> 0 <= i && i < _publication_source.inner().len() ==>
                        _publication_source.inner()[i].channel.0@ != _network_name.inner()
                )]));
            }
            if text.contains("mailboxes . iter_mut") || text.contains("messages . outbox") {
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    _quiet.inner() ==> publications@.len() == 0
                )]));
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    forall<i> 0 <= i && i < publications@.len() ==>
                        publications@[i].1.channel.0@ == _network_name.inner()
                )]));
            }
            if text == "publications" || text == "receivers" {
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    self.time == _route_time.inner() &&
                    mailbox_queues(self.mailboxes@) == _route_queues.inner() &&
                    mailbox_endpoints(self.mailboxes@) == _route_endpoints.inner() &&
                    (_quiet.inner() ==> self.scheduled@ == _route_scheduled.inner())
                )]));
                expr.attrs
                    .push(syn::parse_quote!(#[invariant(self.name@ == _network_name.inner())]));
                expr.attrs.push(syn::parse_quote!(#[invariant(valid_subscribers(subscribers, self.mailboxes@, within_entity))]));
            }
            if text == "receivers" {
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    forall<i> 0 <= i && i < receivers@.len() ==>
                        subscriber_matches((group.deep_model(), publication.channel.deep_model()),
                            receivers@[i], self.mailboxes@, within_entity)
                )]));
                expr.body.stmts.insert(
                    0,
                    syn::parse_quote!(proof_assert! {
                    receivers@[produced.inner().len() - 1] == (index, expected_type)
                };),
                );
            }
            if text == "publications" {
                expr.body.stmts.insert(
                    0,
                    syn::parse_quote!(proof_assert! {
                    _routed_publications.inner()[produced.inner().len() - 1].1 == publication
                };),
                );
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    forall<key, count: u64> sequences@.get(key) == Some(count) ==>
                        count@ <= produced.inner().len()
                )]));
            }
            if text.contains("self . scheduled") {
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    _scheduled_before.inner().len() == 0 ==>
                        mailbox_queues(self.mailboxes@) == _queues_before.inner()
                )]));
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    _scheduled_before.inner() == produced.inner().concat(iter@)
                )]));
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    self.time == _time_before.inner() &&
                    mailbox_endpoints(self.mailboxes@) == _endpoints_before.inner()
                )]));
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    pending@ == pending_messages(produced.inner(), receivers@, _delivery_cutoff_s)
                )]));
                expr.attrs.push(syn::parse_quote!(#[invariant(
                    delivery_queues(_queues_before.inner(), mailbox_queues(self.mailboxes@), produced.inner(),
                        receivers@, _delivery_cutoff_s, self.time.time_s)
                )]));
                expr.body.stmts.insert(
                    0,
                    syn::parse_quote!(proof_assert! {
                    message == produced.inner()[produced.inner().len() - 1] &&
                    produced.inner() == produced.inner().pop_back().push_back(message)
                };),
                );
                expr.body.stmts.insert(1, syn::parse_quote!(proof_assert! {
                    pending@ == pending_messages(produced.inner().pop_back(), receivers@, _delivery_cutoff_s) &&
                    delivery_queues(_queues_before.inner(), mailbox_queues(self.mailboxes@), produced.inner().pop_back(),
                        receivers@, _delivery_cutoff_s, self.time.time_s)
                };));
                expr.body.stmts.insert(2, syn::parse_quote!(proof_assert! {
                    pending_messages(produced.inner(), receivers@, _delivery_cutoff_s) ==
                    if receivers@.get(message.receiver.deep_model()) != None &&
                        !delivery_due(message.envelope.delivered_at_s, _delivery_cutoff_s) {
                        pending_messages(produced.inner().pop_back(), receivers@, _delivery_cutoff_s).push_back(message)
                    } else { pending_messages(produced.inner().pop_back(), receivers@, _delivery_cutoff_s) }
                };));
                expr.body.stmts.insert(3, syn::parse_quote!(proof_assert! {
                    forall<i, channel> delivered_messages(produced.inner(), receivers@, i, channel,
                        _delivery_cutoff_s, self.time.time_s) ==
                    if (match receivers@.get(message.receiver.deep_model()) {
                        Some(index) => index@ == i, None => false,
                    }) && message.channel.deep_model() == channel &&
                        delivery_due(message.envelope.delivered_at_s, _delivery_cutoff_s) {
                        delivered_messages(produced.inner().pop_back(), receivers@, i, channel,
                            _delivery_cutoff_s, self.time.time_s).push_back(delivered_envelope(message.envelope, self.time.time_s))
                    } else { delivered_messages(produced.inner().pop_back(), receivers@, i, channel,
                        _delivery_cutoff_s, self.time.time_s) }
                };));
                expr.attrs.push(
                    syn::parse_quote!(#[invariant(valid_receivers(receivers, self.mailboxes@))]),
                );
                expr.body.stmts.insert(
                    0,
                    syn::parse_quote!(
                        let _iteration_queues = snapshot!(mailbox_queues(self.mailboxes@));
                    ),
                );
                expr.body.stmts.insert(
                    1,
                    syn::parse_quote!(let _original_message = snapshot!(message);),
                );
                expr.body.stmts.insert(
                    0,
                    syn::parse_quote!(let _iteration_mailboxes = snapshot!(self.mailboxes@);),
                );
                expr.body
                    .stmts
                    .push(syn::parse_quote!(let _updated_mailbox = snapshot!(*receiver);));
                expr.body.stmts.push(syn::parse_quote!(proof_assert! {
                    self.mailboxes@.ext_eq(_iteration_mailboxes.inner().set(index@, _updated_mailbox.inner())) &&
                    _updated_mailbox.inner().endpoint.deep_model() == _iteration_mailboxes.inner()[index@].endpoint.deep_model()
                };));
                expr.body.stmts.push(syn::parse_quote!(proof_assert! {
                    let after = mailbox_queues(self.mailboxes@);
                    let original = _original_message.inner();
                    if delivery_due(original.envelope.delivered_at_s, _delivery_cutoff_s) {
                        after.ext_eq(_iteration_queues.inner().set(index@, after[index@])) &&
                        queue_appended(_iteration_queues.inner()[index@].get(original.channel.deep_model()),
                            after[index@].get(original.channel.deep_model()),
                            Seq::singleton(delivered_envelope(original.envelope, self.time.time_s))) &&
                        (forall<channel> channel != original.channel.deep_model() ==>
                            after[index@].get(channel) == _iteration_queues.inner()[index@].get(channel))
                    } else { after.ext_eq(_iteration_queues.inner()) }
                };));
                expr.body.stmts.push(syn::parse_quote!(proof_assert! {
                    mailbox_endpoints(self.mailboxes@).ext_eq(_endpoints_before.inner()) &&
                    delivery_queues(_queues_before.inner(), mailbox_queues(self.mailboxes@),
                        produced.inner(), receivers@, _delivery_cutoff_s, self.time.time_s)
                };));
            }
            syn::visit_mut::visit_expr_for_loop_mut(self, expr);
        }
        fn visit_block_mut(&mut self, block: &mut syn::Block) {
            syn::visit_mut::visit_block_mut(self, block);
            let mut statements = Vec::new();
            for mut statement in std::mem::take(&mut block.stmts) {
                let text = quote!(#statement).to_string();
                if text.starts_with("let mut publications =") {
                    statements.push(syn::parse_quote!(let _route_time = snapshot!(self.time);));
                    statements.push(
                        syn::parse_quote!(let _route_scheduled = snapshot!(self.scheduled@);),
                    );
                    statements.push(syn::parse_quote!(let _route_queues = snapshot!(mailbox_queues(self.mailboxes@));));
                    statements.push(syn::parse_quote!(let _route_endpoints = snapshot!(mailbox_endpoints(self.mailboxes@));));
                    statements.push(syn::parse_quote!(let _route_outboxes = snapshot!(mailbox_outboxes(self.mailboxes@));));
                    let syn::Stmt::Local(local) = &mut statement else {
                        unreachable!()
                    };
                    local.pat = syn::Pat::Type(
                        syn::parse_quote!(mut publications: Vec<(MessageEndpoint, Publication)>),
                    );
                }
                if text.starts_with("let mut other_networks =") {
                    statements.push(syn::parse_quote!(let _publication_source = snapshot!(mailbox.messages.outbox@);));
                    statements.push(syn::parse_quote!(proof_assert! {
                        let _ = sequence_prefixes::<Publication>;
                        let _ = Seq::<Publication>::concat_empty;
                        let _ = quiet_suffix;
                        _quiet.inner() ==> forall<i> 0 <= i && i < _publication_source.inner().len() ==>
                            _publication_source.inner()[i].channel.0@ != _network_name.inner()
                    };));
                }
                if text.starts_with("let subscribers =") {
                    statements.push(syn::parse_quote!(proof_assert! {
                        mailbox_queues(self.mailboxes@).ext_eq(_route_queues.inner()) &&
                        mailbox_endpoints(self.mailboxes@).ext_eq(_route_endpoints.inner())
                    };));
                }
                if text.starts_with("let mailbox = & self . mailboxes [index]") {
                    statements.push(syn::parse_quote!(proof_assert! {
                        subscriber_matches((group.deep_model(), publication.channel.deep_model()),
                            (index, expected_type), self.mailboxes@, within_entity)
                    };));
                }
                if text.starts_with("let inbox = entry_or_insert") {
                    statements.push(
                        syn::parse_quote!(let _inbox_before = snapshot!(receiver.messages.inbox@);),
                    );
                    statements.push(
                        syn::parse_quote!(let _channel = snapshot!(message.channel.deep_model());),
                    );
                    statements
                        .push(syn::parse_quote!(let _envelope = snapshot!(message.envelope);));
                }
                let is_receiver = text.starts_with("let receiver = & mut self . mailboxes");
                let is_link = text.starts_with("let link = Transmission");
                if is_link {
                    self.links += 1;
                    statements.push(syn::parse_quote!(proof_assert! {
                        publication.channel.0@ == self.name@ &&
                        mailbox.messages.subscriptions@.get(publication.channel.deep_model())
                            == Some(payload_kind(publication.value)) &&
                        (!within_entity || (mailbox.endpoint.entity_id == sender.entity_id &&
                            sender.entity_id != None))
                    };));
                }
                if text.starts_with("let mut pending =") {
                    statements.push(syn::parse_quote!(proof_assert! {
                        let _ = empty_delivery;
                        let _ = pending_step;
                        let _ = delivered_step;
                        let _ = delivery_keep_step;
                        let _ = delivery_append_step;
                        let _ = append_suffix::<Envelope>;
                        let _ = sequence_prefixes::<ScheduledMessage>;
                        let _ = mailbox_update;
                        let _ = Seq::<ScheduledMessage>::concat_empty;
                        let _ = sequence_prefixes::<Envelope>;
                        true
                    };));
                    statements.push(syn::parse_quote!(
                        let _delivery_cutoff_s = self.time.time_s + 1e-12;
                    ));
                    statements.push(syn::parse_quote!(
                        let _scheduled_before = snapshot!(self.scheduled@);
                    ));
                    statements.push(syn::parse_quote!(let _time_before = snapshot!(self.time);));
                    statements.push(syn::parse_quote!(
                        let _endpoints_before = snapshot!(mailbox_endpoints(self.mailboxes@));
                    ));
                    statements.push(syn::parse_quote!(
                        let _queues_before = snapshot!(mailbox_queues(self.mailboxes@));
                    ));
                }
                if text.starts_with("let mut pending =") {
                    statements.push(syn::parse_quote!(proof_assert! {
                        receivers@.ext_eq(endpoint_index(mailbox_endpoints(self.mailboxes@))) || self.scheduled@.len() == 0
                    };));
                }
                let is_publications = text.starts_with("let mut publications =");
                if let syn::Stmt::Expr(syn::Expr::ForLoop(expr), _) = &statement {
                    let collection = &expr.expr;
                    if quote!(#collection).to_string() == "publications" {
                        statements.push(
                            syn::parse_quote!(let _routed_publications = snapshot!(publications@);),
                        );
                    }
                }
                statements.push(statement);
                if text.starts_with("* self . scheduled = pending") {
                    statements.push(syn::parse_quote!(proof_assert! {
                        self.scheduled@ == pending_messages(_scheduled_before.inner(), receivers@, _delivery_cutoff_s) &&
                        delivery_queues(_queues_before.inner(), mailbox_queues(self.mailboxes@), _scheduled_before.inner(),
                            receivers@, _delivery_cutoff_s, self.time.time_s)
                    };));
                }
                if is_publications {
                    statements.push(syn::parse_quote!(let _network_name = snapshot!(self.name@);));
                }
                if text.starts_with("let receivers = subscribers") {
                    statements.push(syn::parse_quote!(proof_assert! {
                        matching_entries((group.deep_model(), publication.channel.deep_model()),
                            receivers@, self.mailboxes@, within_entity)
                    };));
                }
                if text.starts_with("inbox . push (message . envelope)") {
                    self.appends += 1;
                    statements.push(syn::parse_quote!(proof_assert! {
                        let previous = match _inbox_before.inner().get(_channel.inner()) {
                            None => Seq::empty(), Some(queue) => queue@,
                        };
                        inbox@ == previous.push_back(_envelope.inner())
                    };));
                    statements.push(syn::parse_quote!(proof_assert! {
                        forall<key> key != _channel.inner() ==>
                            receiver.messages.inbox@.get(key) == _inbox_before.inner().get(key)
                    };));
                    statements.push(syn::parse_quote!(proof_assert! {
                        let previous = match _inbox_before.inner().get(_channel.inner()) {
                            None => Seq::empty(), Some(queue) => queue@,
                        };
                        match receiver.messages.inbox@.get(_channel.inner()) {
                            None => false,
                            Some(queue) => queue@ == previous.push_back(_envelope.inner()),
                        }
                    };));
                }
                if is_link {
                    statements.push(syn::parse_quote!(proof_assert! {
                        link.receiver.deep_model() == mailbox.endpoint.deep_model() &&
                        link.sender.deep_model() == sender.deep_model() &&
                        link.topic@ == publication.channel.1@
                    };));
                }
                if text.starts_with("self . scheduled . push") {
                    self.schedules += 1;
                    statements.push(syn::parse_quote!(proof_assert! {
                        let queued = self.scheduled@[self.scheduled@.len() - 1];
                        queued.receiver.deep_model() == mailbox.endpoint.deep_model() &&
                        queued.channel.deep_model() == publication.channel.deep_model() &&
                        queued.envelope.sender.deep_model() == sender.deep_model() &&
                        queued.envelope.delivered_at_s == delivered_at_s &&
                        same_contents(queued.envelope, publication)
                    };));
                }
                if is_receiver {
                    self.deliveries += 1;
                    statements.push(syn::parse_quote!(proof_assert! {
                        receiver.messages.inbox@ == _iteration_queues.inner()[index@] &&
                        receiver.endpoint.deep_model() == _endpoints_before.inner()[index@]
                    };));
                    statements.push(syn::parse_quote!(proof_assert! {
                        receiver.endpoint.deep_model() == message.receiver.deep_model()
                    };));
                }
            }
            block.stmts = statements;
        }
    }
    #[derive(Default)]
    struct Routing {
        checked: Vec<String>,
    }
    impl VisitMut for Routing {
        fn visit_item_fn_mut(&mut self, item: &mut syn::ItemFn) {
            if item.sig.ident == "subscriber_index" {
                self.checked.push(item.sig.ident.to_string());
                item.attrs
                    .push(syn::parse_quote!(#[requires(mailboxes@.len() < usize::MAX@)]));
                item.attrs.push(syn::parse_quote!(#[ensures(valid_subscribers(result, mailboxes@, within_entity))]));
                IndexLoops { subscriber: true }.visit_block_mut(&mut item.block);
                return;
            }
            if item.sig.ident == "receiver_index" {
                self.checked.push(item.sig.ident.to_string());
                item.attrs
                    .push(syn::parse_quote!(#[requires(mailboxes@.len() < usize::MAX@)]));
                item.attrs
                    .push(syn::parse_quote!(#[ensures(valid_receivers(result, mailboxes@))]));
                item.attrs.push(syn::parse_quote!(#[ensures(result@ == endpoint_index(mailbox_endpoints(mailboxes@)))]));
                item.attrs.push(syn::parse_quote!(#[ensures(
                    forall<i> 0 <= i && i < mailboxes@.len() ==>
                        result@.get(mailboxes@[i].endpoint.deep_model()) != None
                )]));
                IndexLoops { subscriber: false }.visit_block_mut(&mut item.block);
                item.block.stmts.insert(
                    1,
                    syn::parse_quote!(proof_assert! {
                    let _ = endpoint_step;
                    let _ = sequence_prefixes::<(Option<Int>, Seq<char>)>;
                    receivers@.ext_eq(endpoint_index(Seq::empty()))
                };),
                );
                return;
            }
            if item.sig.ident == "message_envelope" {
                self.checked.push(item.sig.ident.to_string());
                item.attrs.extend([
                    syn::parse_quote!(#[ensures(result.sender.deep_model() == sender.deep_model())]),
                    syn::parse_quote!(#[ensures(result.sent_at_s == publication.sent_at_s)]),
                    syn::parse_quote!(#[ensures(result.delivered_at_s == delivered_at_s)]),
                    syn::parse_quote!(#[ensures(result.value == publication.value)]),
                    syn::parse_quote!(#[ensures(result.serialize == publication.serialize)]),
                ]);
                return;
            }
            let boundary = item.sig.ident.to_string();
            assert!(
                [
                    "serialize_payload",
                    "subscription_error",
                    "payload_type_error",
                    "event_ids",
                    "entry_or_insert",
                    "payload_type",
                    "serialized_payload",
                ]
                .contains(&boundary.as_str()),
                "review new routing proof boundary: {boundary}"
            );
            item.attrs.push(syn::parse_quote!(#[trusted]));
            if item.sig.ident == "entry_or_insert" {
                let syn::GenericParam::Type(key) = &mut item.sig.generics.params[0] else {
                    unreachable!()
                };
                key.bounds.push(syn::parse_quote!(DeepModel));
                item.attrs.extend([
                    syn::parse_quote!(#[ensures(*result == match map@.get(key.deep_model()) {
                        Some(value) => value, None => default,
                    })]),
                    syn::parse_quote!(#[ensures((^map)@ == map@.insert(key.deep_model(), ^result))]),
                ]);
            } else if item.sig.ident == "payload_type" {
                item.attrs
                    .push(syn::parse_quote!(#[ensures(result == payload_kind(*value))]));
            }
        }
        fn visit_impl_item_fn_mut(&mut self, item: &mut syn::ImplItemFn) {
            self.checked.push(item.sig.ident.to_string());
            if item.sig.ident == "receive" {
                annotate_receive(item);
            } else if item.sig.ident == "route_links" {
                item.block.stmts.insert(
                    0,
                    syn::parse_quote!(let _quiet = snapshot!(no_publications(*self));),
                );
                item.attrs.push(syn::parse_quote!(#[ensures(match result {
                    Ok(_) => no_publications(*self) ==>
                        (^self).scheduled@ == pending_messages(self.scheduled@,
                            endpoint_index(mailbox_endpoints(self.mailboxes@)), delivery_cutoff(self.time.time_s)) &&
                        delivery_queues(mailbox_queues(self.mailboxes@), mailbox_queues((^self).mailboxes@),
                            self.scheduled@, endpoint_index(mailbox_endpoints(self.mailboxes@)),
                            delivery_cutoff(self.time.time_s), self.time.time_s),
                    Err(_) => true,
                })]));
                item.attrs.push(syn::parse_quote!(#[ensures(match result { Ok(_) => (^self).routed, Err(_) => true })]));
                item.attrs.push(syn::parse_quote!(#[ensures(
                    self.routed ==> match result { Err(_) => ^self == *self, Ok(_) => false }
                )]));
                item.attrs
                    .push(syn::parse_quote!(#[requires(self.mailboxes@.len() < usize::MAX@)]));
                let mut routing = RoutingLoops::default();
                routing.visit_block_mut(&mut item.block);
                assert_eq!(
                    (
                        routing.links,
                        routing.deliveries,
                        routing.appends,
                        routing.schedules
                    ),
                    (1, 0, 0, 1),
                    "routing proof assertions must match the production body"
                );
                item.attrs.push(syn::parse_quote!(#[requires(
                    forall<f: F, t: &Transmission<'_>, r: &PluginRandom> f.precondition((t, r))
                )]));
            } else if item.sig.ident == "deliver_scheduled" {
                item.attrs
                    .push(syn::parse_quote!(#[requires(self.mailboxes@.len() < usize::MAX@)]));
                item.attrs
                    .push(syn::parse_quote!(#[ensures((^self).routed == self.routed)]));
                item.attrs.push(syn::parse_quote!(#[ensures(match result {
                    Ok(_) =>
                        (^self).scheduled@ == pending_messages(self.scheduled@,
                            endpoint_index(mailbox_endpoints(self.mailboxes@)), delivery_cutoff(self.time.time_s)) &&
                        delivery_queues(mailbox_queues(self.mailboxes@), mailbox_queues((^self).mailboxes@),
                            self.scheduled@, endpoint_index(mailbox_endpoints(self.mailboxes@)),
                            delivery_cutoff(self.time.time_s), self.time.time_s),
                    Err(_) => true,
                })]));
                let mut routing = RoutingLoops::default();
                routing.visit_block_mut(&mut item.block);
                assert_eq!(
                    (
                        routing.links,
                        routing.deliveries,
                        routing.appends,
                        routing.schedules
                    ),
                    (0, 1, 1, 0),
                    "delivery proof assertions must match the production body"
                );
            } else {
                item.attrs.push(syn::parse_quote!(#[trusted]));
            }
        }
    }
    for item in &mut file.items {
        if let Item::Struct(item) = item
            && item.ident == "MessageEndpoint"
        {
            item.attrs.retain(|attr| !attr.path().is_ident("derive"));
            item.attrs
                .push(syn::parse_quote!(#[derive(Clone, Debug, PartialEq, Eq)]));
        }
    }
    // Methods outside the checked receive/routing path are not part of the proof.
    for item in &mut file.items {
        if let Item::Impl(item) = item {
            item.items.retain(|member| {
                matches!(member, ImplItem::Fn(method)
                if method.sig.ident == "receive" || method.sig.ident == "route_links" || method.sig.ident == "deliver_scheduled")
            });
        }
    }
    file.items
        .retain(|item| !matches!(item, Item::Mod(module) if module.ident == "tests"));
    file.attrs.clear();
    file.items.retain(
        |item| !matches!(item, Item::Use(u) if quote!(#u).to_string().starts_with("use crate ::")),
    );
    // Keep ensure's branch/return behavior, abstracting only construction of the error.
    for item in &mut file.items {
        if let Item::Use(u) = item
            && quote!(#u).to_string().starts_with("use anyhow ::")
        {
            *u = syn::parse_quote!(
                use anyhow::Result;
            );
        }
    }
    let mut routing = Routing::default();
    routing.visit_file_mut(&mut file);
    routing.checked.sort();
    assert_eq!(
        routing.checked,
        [
            "deliver_scheduled",
            "message_envelope",
            "receive",
            "receiver_index",
            "route_links",
            "subscriber_index"
        ]
    );

    quote! {
        use creusot_std::prelude::*;
        use creusot_std::prelude::{Clone, Default, PartialEq};
        use ripple_core::plugin::{PluginRandom, TruthContacts, StepTime};
        pub use ripple_core::Event;
        #file
    }
}
