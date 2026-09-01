use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};

use dioxus::prelude::rsx;
use dioxus_core::{Element, VNode, VirtualDom, provide_context};
use dioxus_field::{Binding, FieldContext, FieldMetaValues, use_binding, use_field_meta_state};
use dioxus_hooks::use_signal;
use tracing::{
    Event, Level, Metadata,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

/// One tracing event observed by [`CaptureSubscriber`].
#[derive(Clone, Debug)]
struct CapturedEvent {
    target: String,
    level: Level,
    fields: Vec<(String, String)>,
}

impl CapturedEvent {
    fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }

    fn message(&self) -> &str {
        self.field("message").unwrap_or_default()
    }
}

/// A hand-rolled subscriber collecting every event into an `Arc<Mutex<_>>` buffer.
///
/// `tracing::subscriber::with_default` requires `Send + Sync + 'static`, so the buffer cannot be
/// the `Rc<RefCell<_>>` probes the rest of this suite uses.
struct CaptureSubscriber {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl tracing::Subscriber for CaptureSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _attributes: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _id: &Id, _record: &Record<'_>) {}

    fn record_follows_from(&self, _id: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Vec::new();
        event.record(&mut FieldCollector(&mut fields));
        self.events
            .lock()
            .expect("capture buffer should not be poisoned")
            .push(CapturedEvent {
                target: event.metadata().target().to_string(),
                level: *event.metadata().level(),
                fields,
            });
    }

    fn enter(&self, _id: &Id) {}

    fn exit(&self, _id: &Id) {}
}

struct FieldCollector<'a>(&'a mut Vec<(String, String)>);

impl Visit for FieldCollector<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .push((field.name().to_string(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_string(), value.to_string()));
    }
}

/// Runs `operation` under a capturing subscriber and returns its result with this crate's events.
///
/// Filtering by target matters: dioxus-core emits its own ERROR events in the same catch path
/// when a component panics, and those must not count against this crate's cadence.
fn capture_crate_events_during<R>(operation: impl FnOnce() -> R) -> (R, Vec<CapturedEvent>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = CaptureSubscriber {
        events: Arc::clone(&events),
    };

    let result = tracing::subscriber::with_default(subscriber, operation);

    let events = events
        .lock()
        .expect("capture buffer should not be poisoned");
    let events = events
        .iter()
        .filter(|event| event.target == "dioxus_field")
        .cloned()
        .collect();

    (result, events)
}

#[derive(Clone)]
struct ContextProbe {
    build: fn() -> FieldContext,
    cell: Rc<RefCell<Option<FieldContext>>>,
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn context_probe_app(probe: ContextProbe) -> Element {
    probe.cell.borrow_mut().replace((probe.build)());
    VNode::empty()
}

/// Builds `build`'s context inside a rebuilt `VirtualDom` and hands both back.
///
/// The returned dom keeps the context's signals alive, so assertions run in the test body — where
/// a failure fails the test instead of being swallowed by dioxus's render `catch_unwind` — while
/// metadata stays readable. Dropping the dom first exercises the dropped-owner path instead.
fn realize_context(build: fn() -> FieldContext) -> (VirtualDom, FieldContext) {
    let cell: Rc<RefCell<Option<FieldContext>>> = Rc::default();
    let mut dom = VirtualDom::new_with_props(
        context_probe_app,
        ContextProbe {
            build,
            cell: Rc::clone(&cell),
        },
    );
    dom.rebuild_in_place();
    let context = cell
        .borrow_mut()
        .take()
        .expect("probe should contain field context");

    (dom, context)
}

/// Resolves `i32` from `context`, catching the enforcement panic and returning its message.
fn resolve_panic_message(context: &FieldContext) -> String {
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Binding<i32> = context.resolve();
    }))
    .expect_err("resolve should panic");

    payload
        .downcast_ref::<String>()
        .expect("panic payload should be the formatted message")
        .clone()
}

fn meta_bearing_wrong_type_context() -> FieldContext {
    let signal = use_signal(|| 5i64);
    let meta = use_field_meta_state(FieldMetaValues {
        id: Some(Rc::from("amount")),
        name: Some(Rc::from("amount-field")),
        ..FieldMetaValues::default()
    });

    FieldContext::new(Binding::<i64>::from(signal)).with_meta(meta)
}

#[test]
fn resolve_panic_appends_field_identity_after_the_verbatim_sentence() {
    let (_dom, context) = realize_context(meta_bearing_wrong_type_context);

    let (message, events) = capture_crate_events_during(|| resolve_panic_message(&context));

    assert!(
        message.starts_with(
            "Field Context contains a binding for i64, but a binding for i32 was requested"
        ),
        "panic message should keep the verbatim sentence as prefix: {message}"
    );
    assert!(
        message.ends_with(" (field id: amount, field name: amount-field)"),
        "panic message should append the field identity: {message}"
    );

    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.level, Level::ERROR);
    assert_eq!(event.message(), "Field Context binding type mismatch");
    assert_eq!(event.field("actual"), Some("i64"));
    assert_eq!(event.field("requested"), Some("i32"));
    assert_eq!(event.field("field_id"), Some("amount"));
    assert_eq!(event.field("field_name"), Some("amount-field"));
}

#[test]
fn resolve_after_the_dom_drops_omits_identity_without_a_second_panic() {
    let (dom, context) = realize_context(meta_bearing_wrong_type_context);
    drop(dom);

    let message = resolve_panic_message(&context);

    assert_eq!(
        message,
        "Field Context contains a binding for i64, but a binding for i32 was requested"
    );
}

#[test]
fn resolving_a_context_without_value_binding_emits_a_requested_type_event() {
    fn meta_only_context() -> FieldContext {
        let meta = use_field_meta_state(FieldMetaValues {
            id: Some(Rc::from("amount")),
            ..FieldMetaValues::default()
        });

        FieldContext::empty().with_meta(meta)
    }

    let (_dom, context) = realize_context(meta_only_context);

    let (message, events) = capture_crate_events_during(|| resolve_panic_message(&context));

    assert!(
        message.starts_with("Field Context contains no value binding"),
        "panic message should keep the verbatim sentence as prefix: {message}"
    );
    assert!(
        message.ends_with(" (field id: amount)"),
        "panic message should append the available identity parts only: {message}"
    );

    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.level, Level::ERROR);
    assert_eq!(event.message(), "Field Context contains no value binding");
    assert_eq!(event.field("requested"), Some("i32"));
    assert_eq!(event.field("field_id"), Some("amount"));
    assert_eq!(event.field("actual"), None);
    assert_eq!(event.field("field_name"), None);
}

#[test]
fn multi_type_probing_through_try_resolve_emits_no_events() {
    #[derive(Clone, Copy)]
    struct CheckboxState;

    fn bool_context() -> FieldContext {
        let signal = use_signal(|| true);

        FieldContext::new(Binding::<bool>::from(signal))
    }

    let (_dom, context) = realize_context(bool_context);

    let ((), events) = capture_crate_events_during(|| {
        assert!(context.try_resolve::<CheckboxState>().is_err());
        assert!(matches!(context.try_resolve::<bool>(), Ok(Some(_))));
    });

    assert!(events.is_empty());
}

#[allow(non_snake_case)]
fn StringControl() -> Element {
    let _ = use_binding::<String>(None, String::new());
    VNode::empty()
}

fn mismatch_producer_app() -> Element {
    let signal = use_signal(|| Option::<String>::None);
    let binding: Binding<Option<String>> = signal.into();
    let meta = use_field_meta_state(FieldMetaValues {
        id: Some(Rc::from("amount")),
        name: Some(Rc::from("amount-field")),
        ..FieldMetaValues::default()
    });
    provide_context(FieldContext::new(binding).with_meta(meta));

    rsx! { StringControl {} }
}

#[test]
fn wrong_context_value_type_in_render_emits_one_diagnostic_event() {
    let ((), events) = capture_crate_events_during(|| {
        VirtualDom::new(mismatch_producer_app).rebuild_in_place();
    });

    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.level, Level::ERROR);
    assert_eq!(event.message(), "Field Context binding type mismatch");
    assert_eq!(
        event.field("actual"),
        Some("core::option::Option<alloc::string::String>")
    );
    assert_eq!(event.field("requested"), Some("alloc::string::String"));
    assert_eq!(event.field("field_id"), Some("amount"));
    assert_eq!(event.field("field_name"), Some("amount-field"));
}
