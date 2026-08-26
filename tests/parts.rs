use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use dioxus::prelude::*;
use dioxus_core::AttributeValue;
use dioxus_field::{
    Field, FieldContext, FieldControlOptions, FieldDescription, FieldError, FieldMeta,
    FieldMetaValues, FocusRequest, Label, use_field_meta_state, use_focus_registration,
};

struct InvalidProbe {
    meta: RefCell<Option<FieldMeta>>,
    parent_renders: Cell<usize>,
}

struct RegistrationProbe {
    meta: RefCell<Option<FieldMeta>>,
    show_parts: RefCell<Option<Signal<bool>>>,
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn registration_app(probe: Rc<RegistrationProbe>) -> Element {
    let meta = use_field_meta_state(FieldMetaValues::default());
    let show_parts = use_signal(|| true);
    probe.meta.borrow_mut().replace(meta);
    probe.show_parts.borrow_mut().replace(show_parts);

    rsx! {
        Field {
            context: FieldContext::empty().with_meta(meta),
            if show_parts() {
                FieldDescription { id: "email-help", "Use a work address" }
                FieldError { id: "email-error" }
            }
        }
    }
}

fn attribute_text(attributes: &[Attribute], name: &str) -> Option<String> {
    attributes
        .iter()
        .find(|attribute| attribute.name == name)
        .and_then(|attribute| match &attribute.value {
            AttributeValue::Text(value) => Some(value.clone()),
            _ => None,
        })
}

#[test]
fn description_and_error_ids_follow_part_mount_and_drop() {
    let probe = Rc::new(RegistrationProbe {
        meta: RefCell::new(None),
        show_parts: RefCell::new(None),
    });
    let mut dom = VirtualDom::new_with_props(registration_app, Rc::clone(&probe));
    dom.rebuild_in_place();

    let meta = probe.meta.borrow().expect("app should expose field meta");
    let attributes = meta.attributes_for(&FieldControlOptions::new().invalid(Some(true)));
    assert_eq!(
        attribute_text(&attributes, "aria-describedby").as_deref(),
        Some("email-help email-error"),
        "error ids join the description ids while invalid, since aria-errormessage support is uneven"
    );
    assert_eq!(
        attribute_text(&attributes, "aria-errormessage").as_deref(),
        Some("email-error")
    );

    probe
        .show_parts
        .borrow_mut()
        .as_mut()
        .expect("app should expose visibility signal")
        .set(false);
    dom.render_immediate_to_vec();

    let attributes = meta.attributes_for(&FieldControlOptions::new().invalid(Some(true)));
    assert_eq!(attribute_text(&attributes, "aria-describedby"), None);
    assert_eq!(attribute_text(&attributes, "aria-errormessage"), None);
}

#[test]
fn field_parts_render_standalone_with_explicit_or_default_metadata() {
    fn app() -> Element {
        let meta = use_field_meta_state(FieldMetaValues {
            id: Some(Rc::from("email")),
            invalid: Some(true),
            errors: vec![Rc::from("Required")],
            ..FieldMetaValues::default()
        });

        rsx! {
            Label { id: Rc::from("email-label"), meta, "Email" }
            FieldDescription { id: "standalone-help", meta, "Use a work address" }
            FieldError { id: "standalone-error", meta }
        }
    }

    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    assert_eq!(
        dioxus_ssr::render(&dom),
        "<label id=\"email-label\" for=\"email\" data-invalid=\"true\">Email</label><div id=\"standalone-help\" data-invalid=\"true\">Use a work address</div><div id=\"standalone-error\" aria-live=\"polite\" data-invalid=\"true\"><div>Required</div></div>"
    );
}

struct ChangingMetaProbe {
    first: RefCell<Option<FieldMeta>>,
    second: RefCell<Option<FieldMeta>>,
    use_second: RefCell<Option<Signal<bool>>>,
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn changing_meta_app(probe: Rc<ChangingMetaProbe>) -> Element {
    let first = use_field_meta_state(FieldMetaValues::default());
    let second = use_field_meta_state(FieldMetaValues::default());
    let use_second = use_signal(|| false);
    probe.first.borrow_mut().replace(first);
    probe.second.borrow_mut().replace(second);
    probe.use_second.borrow_mut().replace(use_second);
    let meta = if use_second() { second } else { first };

    rsx! { FieldDescription { id: "changing-help", meta, "Help" } }
}

#[test]
fn mounted_part_registration_follows_an_explicit_meta_change() {
    let probe = Rc::new(ChangingMetaProbe {
        first: RefCell::new(None),
        second: RefCell::new(None),
        use_second: RefCell::new(None),
    });
    let mut dom = VirtualDom::new_with_props(changing_meta_app, Rc::clone(&probe));
    dom.rebuild_in_place();
    let first = probe.first.borrow().expect("app should expose first meta");
    let second = probe
        .second
        .borrow()
        .expect("app should expose second meta");

    assert_eq!(
        attribute_text(&first.attributes(), "aria-describedby").as_deref(),
        Some("changing-help")
    );
    assert_eq!(
        attribute_text(&second.attributes(), "aria-describedby"),
        None
    );

    probe
        .use_second
        .borrow_mut()
        .as_mut()
        .expect("app should expose meta selector")
        .set(true);
    dom.render_immediate_to_vec();

    assert_eq!(
        attribute_text(&first.attributes(), "aria-describedby"),
        None
    );
    assert_eq!(
        attribute_text(&second.attributes(), "aria-describedby").as_deref(),
        Some("changing-help")
    );
}

#[derive(PartialEq)]
struct ChangingFocusProbe {
    show_requester: RefCell<Option<Signal<bool>>>,
    request: RefCell<Option<FocusRequest>>,
    calls: Cell<usize>,
    widget_renders: Cell<usize>,
}

#[derive(Clone, Props, PartialEq)]
struct FocusWidgetProps {
    probe: Rc<ChangingFocusProbe>,
}

#[allow(non_snake_case)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Dioxus components receive their generated properties by value"
)]
fn FocusWidget(props: FocusWidgetProps) -> Element {
    props
        .probe
        .widget_renders
        .set(props.probe.widget_renders.get() + 1);
    let probe = Rc::clone(&props.probe);
    // `use_callback` keeps one callback identity across renders. `Callback::new` here would
    // re-register on every render and steal the field's focus slot from a sibling widget.
    use_focus_registration(use_callback(move |()| {
        probe.calls.set(probe.calls.get() + 1);
    }));

    VNode::empty()
}

#[derive(Clone, Props, PartialEq)]
struct FocusRequesterProps {
    probe: Rc<ChangingFocusProbe>,
}

#[allow(non_snake_case)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Dioxus components receive their generated properties by value"
)]
fn FocusRequester(props: FocusRequesterProps) -> Element {
    let request = dioxus_field::use_focus_request();
    props.probe.request.borrow_mut().replace(request);

    VNode::empty()
}

fn changing_focus_app(probe: Rc<ChangingFocusProbe>) -> Element {
    let show_requester = use_signal(|| false);
    probe.show_requester.borrow_mut().replace(show_requester);

    rsx! {
        Field {
            context: FieldContext::empty(),
            FocusWidget { probe: Rc::clone(&probe) }
            if show_requester() {
                FocusRequester { probe }
            }
        }
    }
}

#[test]
fn focus_registration_follows_the_resolved_context_slot() {
    let probe = Rc::new(ChangingFocusProbe {
        show_requester: RefCell::new(None),
        request: RefCell::new(None),
        calls: Cell::new(0),
        widget_renders: Cell::new(0),
    });
    let mut dom = VirtualDom::new_with_props(changing_focus_app, Rc::clone(&probe));
    dom.rebuild_in_place();
    assert!(probe.request.borrow().is_none());

    probe
        .show_requester
        .borrow_mut()
        .as_mut()
        .expect("app should expose requester visibility")
        .set(true);
    dom.render_immediate_to_vec();

    assert!(
        probe
            .request
            .borrow()
            .as_ref()
            .expect("widget should expose updated focus request")
            .request()
    );
    assert_eq!(probe.calls.get(), 1);
    assert_eq!(probe.widget_renders.get(), 1);
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn reactive_error_app(probe: Rc<InvalidProbe>) -> Element {
    probe.parent_renders.set(probe.parent_renders.get() + 1);
    let meta = use_field_meta_state(FieldMetaValues::default());
    probe.meta.borrow_mut().replace(meta);

    rsx! {
        Field {
            context: FieldContext::empty().with_meta(meta),
            FieldError { id: "email-error" }
        }
    }
}

#[test]
fn invalid_changes_reach_field_error_without_rerendering_the_field_parent() {
    let probe = Rc::new(InvalidProbe {
        meta: RefCell::new(None),
        parent_renders: Cell::new(0),
    });
    let mut dom = VirtualDom::new_with_props(reactive_error_app, Rc::clone(&probe));
    dom.rebuild_in_place();

    assert_eq!(
        dioxus_ssr::render(&dom),
        "<div><div id=\"email-error\" aria-live=\"polite\"></div></div>",
        "the live region must already be in the accessibility tree before the first error arrives"
    );
    let mut meta = probe.meta.borrow().expect("app should expose field meta");
    meta.set_errors(vec![Rc::from("Enter a valid email")]);
    dom.render_immediate_to_vec();

    assert_eq!(probe.parent_renders.get(), 1);
    assert_eq!(
        dioxus_ssr::render(&dom),
        "<div><div id=\"email-error\" aria-live=\"polite\" data-invalid=\"true\"><div>Enter a valid email</div></div></div>"
    );
}

struct FocusOwnershipProbe {
    first_calls: Cell<usize>,
    second_calls: Cell<usize>,
    nudge: RefCell<Option<Signal<usize>>>,
    request: RefCell<Option<FocusRequest>>,
}

impl FocusOwnershipProbe {
    fn calls(&self) -> (usize, usize) {
        (self.first_calls.get(), self.second_calls.get())
    }

    fn request(&self) -> bool {
        self.request
            .borrow()
            .as_ref()
            .expect("a widget should expose the field's focus request")
            .request()
    }
}

impl PartialEq for FocusOwnershipProbe {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

#[derive(Clone, Props, PartialEq)]
struct OwnedFocusWidgetProps {
    probe: Rc<FocusOwnershipProbe>,
    first: bool,
    /// An unrelated value that re-renders this widget without changing anything about focus.
    #[props(default)]
    nudge: usize,
}

#[allow(non_snake_case)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Dioxus components receive their generated properties by value"
)]
fn OwnedFocusWidget(props: OwnedFocusWidgetProps) -> Element {
    let probe = Rc::clone(&props.probe);
    let first = props.first;
    let request = use_focus_registration(use_callback(move |()| {
        let calls = if first {
            &probe.first_calls
        } else {
            &probe.second_calls
        };
        calls.set(calls.get() + 1);
    }));
    props.probe.request.borrow_mut().replace(request);

    VNode::empty()
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn focus_ownership_app(probe: Rc<FocusOwnershipProbe>) -> Element {
    let nudge = use_signal(|| 0);
    probe.nudge.borrow_mut().replace(nudge);

    rsx! {
        Field {
            context: FieldContext::empty(),
            OwnedFocusWidget { probe: Rc::clone(&probe), first: true, nudge: nudge() }
            OwnedFocusWidget { probe, first: false }
        }
    }
}

#[test]
fn focus_ownership_survives_an_unrelated_rerender_of_a_sibling_widget() {
    let probe = Rc::new(FocusOwnershipProbe {
        first_calls: Cell::new(0),
        second_calls: Cell::new(0),
        nudge: RefCell::new(None),
        request: RefCell::new(None),
    });
    let mut dom = VirtualDom::new_with_props(focus_ownership_app, Rc::clone(&probe));
    dom.rebuild_in_place();

    assert!(probe.request());
    assert_eq!(
        probe.calls(),
        (0, 1),
        "the last widget to register owns the field's focus slot"
    );

    probe
        .nudge
        .borrow_mut()
        .as_mut()
        .expect("app should expose the nudge signal")
        .set(1);
    dom.render_immediate_to_vec();

    assert!(probe.request());
    assert_eq!(
        probe.calls(),
        (0, 2),
        "re-rendering a sibling must not move the field's focus slot to it"
    );
}

#[test]
fn an_explicit_required_prop_overrides_the_metadata_state_on_every_part() {
    fn app() -> Element {
        let meta = use_field_meta_state(FieldMetaValues {
            id: Some(Rc::from("email")),
            required: true,
            invalid: Some(true),
            errors: vec![Rc::from("Required")],
            ..FieldMetaValues::default()
        });

        rsx! {
            Field { context: FieldContext::empty().with_meta(meta),
                Label { id: Rc::from("email-label"), required: false, "Email" }
                FieldDescription { id: "email-help", required: false, "Use a work address" }
                FieldError { id: "email-error", required: false }
            }
        }
    }

    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    let rendered = dioxus_ssr::render(&dom);

    assert!(
        !rendered.contains("data-required"),
        "an explicit required prop must suppress the metadata's own required state, got {rendered}"
    );
    assert_eq!(
        rendered.matches("data-invalid=\"true\"").count(),
        3,
        "the states left unoverridden must still come from the metadata, got {rendered}"
    );
}
