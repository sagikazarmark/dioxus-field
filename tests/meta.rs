use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_core::AttributeValue;
use dioxus_field::{FieldMetaOverrides, FieldMetaValues, FocusRequest, use_field_meta_state};

fn attribute_text(attributes: &[Attribute], name: &str) -> Option<String> {
    attributes
        .iter()
        .find(|attribute| attribute.name == name)
        .and_then(|attribute| match &attribute.value {
            AttributeValue::Text(value) => Some(value.clone()),
            AttributeValue::Bool(value) => Some(value.to_string()),
            _ => None,
        })
}

#[test]
fn field_meta_attributes_derive_accessibility_and_state_with_flag_overrides() {
    #[allow(
        clippy::needless_pass_by_value,
        reason = "VirtualDom entrypoints receive their root properties by value"
    )]
    fn app(attributes: Rc<std::cell::RefCell<Vec<Attribute>>>) -> Element {
        let mut meta = use_field_meta_state(FieldMetaValues {
            id: Some(Rc::from("account-email")),
            name: Some(Rc::from("email")),
            required: true,
            disabled: true,
            errors: vec![Rc::from("Enter a valid email")],
            touched: true,
            dirty: true,
            ..FieldMetaValues::default()
        });
        let _description = meta.register_description_id(Rc::from("email-help"));
        let _error = meta.register_error_id(Rc::from("email-error"));

        attributes
            .borrow_mut()
            .extend(meta.attributes_with(FieldMetaOverrides {
                invalid: Some(false),
                disabled: Some(false),
            }));

        VNode::empty()
    }

    let attributes = Rc::new(std::cell::RefCell::new(Vec::new()));
    VirtualDom::new_with_props(app, Rc::clone(&attributes)).rebuild_in_place();
    let attributes = attributes.borrow();

    assert_eq!(
        attribute_text(&attributes, "id").as_deref(),
        Some("account-email")
    );
    assert_eq!(
        attribute_text(&attributes, "name").as_deref(),
        Some("email")
    );
    assert_eq!(
        attribute_text(&attributes, "aria-invalid").as_deref(),
        Some("false")
    );
    assert_eq!(
        attribute_text(&attributes, "aria-describedby").as_deref(),
        Some("email-help")
    );
    assert_eq!(attribute_text(&attributes, "aria-errormessage"), None);
    assert_eq!(attribute_text(&attributes, "disabled"), None);
    assert_eq!(
        attribute_text(&attributes, "data-touched").as_deref(),
        Some("true")
    );
    assert_eq!(
        attribute_text(&attributes, "data-dirty").as_deref(),
        Some("true")
    );
}

#[test]
fn focus_request_tracks_the_current_lifecycle_registration() {
    #[allow(
        clippy::needless_pass_by_value,
        reason = "VirtualDom entrypoints receive their root properties by value"
    )]
    fn app(requests: Rc<std::cell::Cell<usize>>) -> Element {
        let first_requests = Rc::clone(&requests);
        let second_requests = Rc::clone(&requests);
        let request = FocusRequest::default();
        let first = request.register(Callback::new(move |()| {
            first_requests.set(first_requests.get() + 1);
        }));

        assert!(request.request());
        let second = request.register(Callback::new(move |()| {
            second_requests.set(second_requests.get() + 10);
        }));
        drop(first);
        assert!(request.request());
        drop(second);
        assert!(!request.request());

        VNode::empty()
    }

    let requests = Rc::new(std::cell::Cell::new(0));
    VirtualDom::new_with_props(app, Rc::clone(&requests)).rebuild_in_place();
    assert_eq!(requests.get(), 11);
}
