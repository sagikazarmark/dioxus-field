use std::{cell::RefCell, rc::Rc};

use dioxus::prelude::*;
use dioxus_core::{AttributeValue, Mutation};
use dioxus_field::{
    Field, FieldContext, FieldControlOptions, FieldDescription, FieldError, FieldMeta,
    FieldMetaValues, FieldSurface, Label, use_field_meta_state,
};

#[derive(Default)]
struct MetaProbe(RefCell<Option<FieldMeta>>);

impl MetaProbe {
    fn meta(&self) -> FieldMeta {
        self.0.borrow().expect("app should expose field metadata")
    }
}

fn saturated_values() -> FieldMetaValues {
    FieldMetaValues {
        id: Some(Rc::from("account-email")),
        name: Some(Rc::from("email")),
        required: true,
        disabled: true,
        invalid: Some(true),
        errors: vec![Rc::from("Enter a valid email")],
        touched: true,
        dirty: true,
    }
}

/// Every `SetAttribute` mutation, in the order the renderer would apply it.
fn attribute_edits(mutations: &dioxus_core::Mutations) -> Vec<(&'static str, Option<String>)> {
    mutations
        .edits
        .iter()
        .filter_map(|edit| match edit {
            Mutation::SetAttribute { name, value, .. } => Some((
                *name,
                match value {
                    AttributeValue::Text(text) => Some(text.clone()),
                    AttributeValue::Bool(value) => Some(value.to_string()),
                    AttributeValue::None => None,
                    _ => Some(String::from("<other>")),
                },
            )),
            _ => None,
        })
        .collect()
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn spread_control_app(probe: Rc<MetaProbe>) -> Element {
    let meta = use_field_meta_state(saturated_values());
    probe.0.borrow_mut().replace(meta);

    rsx! {
        input { ..meta.attributes() }
    }
}

#[test]
fn a_flag_flip_on_a_spread_control_only_removes_that_flag() {
    let probe = Rc::new(MetaProbe::default());
    let mut dom = VirtualDom::new_with_props(spread_control_app, Rc::clone(&probe));
    dom.rebuild_in_place();

    let mut meta = probe.meta();
    meta.set_required(false);
    let edits = attribute_edits(&dom.render_immediate_to_vec());

    assert_eq!(
        edits,
        vec![("data-required", None), ("required", None)],
        "flipping one flag must emit exactly the mutations for that flag"
    );
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn parts_app(probe: Rc<MetaProbe>) -> Element {
    let meta = use_field_meta_state(saturated_values());
    probe.0.borrow_mut().replace(meta);

    rsx! {
        Field { context: FieldContext::empty().with_meta(meta),
            Label { "Email" }
            FieldDescription { id: "email-help", "Use a work address" }
            FieldError { id: "email-error" }
        }
    }
}

#[test]
fn a_flag_flip_on_each_part_only_removes_that_flag() {
    let probe = Rc::new(MetaProbe::default());
    let mut dom = VirtualDom::new_with_props(parts_app, Rc::clone(&probe));
    dom.rebuild_in_place();

    let mut meta = probe.meta();
    meta.set_required(false);
    let edits = attribute_edits(&dom.render_immediate_to_vec());

    assert_eq!(
        edits,
        vec![
            ("data-required", None),
            ("data-required", None),
            ("data-required", None),
        ],
        "each part must emit exactly the mutation for the flipped flag"
    );
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn options_app(
    input: Rc<(
        FieldMetaValues,
        FieldControlOptions,
        RefCell<Vec<Attribute>>,
    )>,
) -> Element {
    let meta = use_field_meta_state(input.0.clone());
    *input.2.borrow_mut() = meta.attributes_for(&input.1);

    VNode::empty()
}

/// Renders one set of metadata through one set of control options and describes what it emitted.
fn emitted(values: FieldMetaValues, options: FieldControlOptions) -> Vec<String> {
    let input = Rc::new((values, options, RefCell::new(Vec::new())));
    VirtualDom::new_with_props(options_app, Rc::clone(&input)).rebuild_in_place();
    let attributes = input.2.borrow();

    attributes
        .iter()
        .map(|attribute| match &attribute.value {
            AttributeValue::Text(text) => format!("{}={text}", attribute.name),
            AttributeValue::Bool(value) => format!("{}={value}", attribute.name),
            _ => attribute.name.to_string(),
        })
        .collect()
}

#[test]
fn the_surface_decides_which_spelling_of_each_attribute_is_emitted() {
    let values = FieldMetaValues {
        id: Some(Rc::from("terms")),
        name: Some(Rc::from("terms")),
        required: true,
        disabled: true,
        ..FieldMetaValues::default()
    };

    assert_eq!(
        emitted(
            values.clone(),
            FieldControlOptions::new().surface(FieldSurface::NATIVE)
        ),
        [
            "aria-invalid=false",
            "data-disabled=true",
            "data-required=true",
            "disabled=true",
            "id=terms",
            "name=terms",
            "required=true",
        ],
        "an input spells every axis natively"
    );

    assert_eq!(
        emitted(
            values.clone(),
            FieldControlOptions::new().surface(FieldSurface::BUTTON_WIDGET)
        ),
        [
            "aria-invalid=false",
            "aria-required=true",
            "data-disabled=true",
            "data-required=true",
            "disabled=true",
            "id=terms",
            "name=terms",
        ],
        "a button[role=switch] takes native disabled but has no native required"
    );

    assert_eq!(
        emitted(
            values,
            FieldControlOptions::new().surface(FieldSurface::ARIA_WIDGET)
        ),
        [
            "aria-disabled=true",
            "aria-invalid=false",
            "aria-required=true",
            "data-disabled=true",
            "data-required=true",
            "id=terms",
        ],
        "a div[role=radiogroup] carries no native attribute and no name"
    );
}

#[test]
fn an_override_replaces_the_metadata_state_instead_of_appending_to_it() {
    let values = FieldMetaValues {
        id: Some(Rc::from("email")),
        name: Some(Rc::from("email")),
        required: true,
        disabled: true,
        ..FieldMetaValues::default()
    };

    assert_eq!(
        emitted(
            values,
            FieldControlOptions::new()
                .required(Some(false))
                .disabled(Some(false))
                .invalid(Some(true))
                .name(Some(Rc::from("newsletter")))
                .id(Some(Rc::from("newsletter-email"))),
        ),
        [
            "aria-invalid=true",
            "data-invalid=true",
            "id=newsletter-email",
            "name=newsletter",
        ],
        "an overridden state is never emitted from the metadata as well"
    );
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn generated_id_app(probe: Rc<MetaProbe>) -> Element {
    let meta = use_field_meta_state(FieldMetaValues::default());
    probe.0.borrow_mut().replace(meta);

    rsx! {
        Field { context: FieldContext::empty().with_meta(meta),
            Label { "Email" }
            input { ..meta.attributes() }
        }
    }
}

#[test]
fn a_field_without_a_producer_id_is_still_labelled_and_addressable() {
    let probe = Rc::new(MetaProbe::default());
    let mut dom = VirtualDom::new_with_props(generated_id_app, Rc::clone(&probe));
    dom.rebuild_in_place();
    // The parts register their ids while they render, so the control picks the references up on
    // the render that follows.
    dom.render_immediate_to_vec();

    let control_id = probe.meta().id();
    let rendered = dioxus_ssr::render(&dom);

    assert!(
        rendered.contains(&format!("for=\"{control_id}\"")),
        "the label must point at the generated control id, got {rendered}"
    );
    assert!(
        rendered.contains(&format!("id=\"{control_id}\"")),
        "the control must render the generated id, got {rendered}"
    );
    assert!(
        rendered.contains("aria-labelledby=\"dxf-label-"),
        "the control must reference the label's own id, got {rendered}"
    );
}

#[test]
fn a_forwarded_attribute_wins_its_name_and_is_never_emitted_twice() {
    fn app() -> Element {
        let meta = use_field_meta_state(FieldMetaValues {
            invalid: Some(true),
            ..FieldMetaValues::default()
        });

        rsx! {
            Field {
                context: FieldContext::empty().with_meta(meta),
                attributes: vec![
                    Attribute::new("lang", "en", None, false),
                    Attribute::new("dir", "rtl", None, false),
                    Attribute::new("dir", "ltr", None, false),
                ],
                FieldDescription {
                    id: "email-help",
                    attributes: vec![
                        Attribute::new("data-invalid", "false", None, false),
                        Attribute::new("title", "first", None, false),
                        Attribute::new("title", "last", None, false),
                    ],
                    "Use a work address"
                }
            }
        }
    }

    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    let rendered = dioxus_ssr::render(&dom);

    assert!(
        rendered.contains("data-invalid=\"false\""),
        "a forwarded attribute is appended last, so it wins its name, got {rendered}"
    );
    assert_eq!(
        rendered.matches("data-invalid").count(),
        1,
        "the part's own state attribute must not survive alongside it, got {rendered}"
    );
    assert!(
        rendered.contains("title=\"last\"") && rendered.matches("title=").count() == 1,
        "repeats within the forwarded list resolve last-wins too, got {rendered}"
    );
    assert!(
        rendered.starts_with("<div dir=\"ltr\" lang=\"en\">"),
        "`Field` sorts and dedupes its own forwarded list too, got {rendered}"
    );
}

#[test]
fn merging_groups_resolves_each_name_to_the_last_group_that_set_it() {
    let meta = vec![
        Attribute::new("id", "from-meta", None, false),
        Attribute::new("name", "from-meta", None, false),
    ];
    let base = vec![
        Attribute::new("name", "from-base", None, false),
        Attribute::new("class", "from-base", None, false),
    ];
    let explicit = vec![Attribute::new("name", "from-explicit", None, false)];

    // The group order the registry controls use: base sits between the metadata and the explicit
    // props, so it outranks the metadata and loses to the explicit props.
    let merged = dioxus_field::merge_attributes(vec![meta, base, explicit]);

    assert_eq!(
        merged
            .iter()
            .map(|attribute| match &attribute.value {
                AttributeValue::Text(text) => format!("{}={text}", attribute.name),
                _ => attribute.name.to_string(),
            })
            .collect::<Vec<_>>(),
        ["class=from-base", "id=from-meta", "name=from-explicit",],
        "each name resolves to the last group that set it, and the result stays sorted"
    );
}

#[test]
fn merging_keeps_one_entry_per_name_and_namespace() {
    let merged = dioxus_field::merge_attributes(vec![
        vec![
            Attribute::new("style", "red", Some("color"), false),
            Attribute::new("style", "1px", Some("border-width"), false),
        ],
        vec![Attribute::new("style", "blue", Some("color"), false)],
    ]);

    assert_eq!(
        merged.len(),
        2,
        "a namespace makes two entries distinct even though they share a name"
    );
    assert_eq!(
        merged
            .iter()
            .find(|attribute| attribute.namespace == Some("color"))
            .map(|attribute| &attribute.value),
        Some(&AttributeValue::Text(String::from("blue"))),
        "the later group still wins within one namespace"
    );
}

#[test]
fn merging_an_appended_group_preserves_the_attributes_for_guarantee() {
    fn app() -> Element {
        let meta = use_field_meta_state(FieldMetaValues {
            id: Some(Rc::from("email")),
            required: true,
            ..FieldMetaValues::default()
        });
        let merged = dioxus_field::merge_attributes(vec![
            meta.attributes(),
            vec![Attribute::new("data-focused", "true", None, false)],
        ]);

        rsx! {
            input { ..merged }
        }
    }

    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();

    assert_eq!(
        dioxus_ssr::render(&dom),
        "<input aria-invalid=\"false\" data-focused=\"true\" data-required=\"true\" id=\"email\" required=true/>",
        "the merged result is sorted, so it is safe to spread"
    );
}

#[test]
fn merging_concatenates_class_instead_of_replacing_it() {
    let merged = dioxus_field::merge_attributes(vec![
        vec![Attribute::new("class", "radio radio-primary", None, false)],
        vec![
            Attribute::new("class", "mt-2", None, false),
            Attribute::new("id", "agree", None, false),
        ],
    ]);

    assert_eq!(
        merged
            .iter()
            .find(|attribute| attribute.name == "class")
            .map(|attribute| &attribute.value),
        Some(&AttributeValue::Text(String::from(
            "radio radio-primary mt-2"
        ))),
        "a widget's own classes must survive a caller's, or spreading silently unstyles the widget"
    );
    assert_eq!(
        merged.len(),
        2,
        "concatenating still leaves exactly one class entry"
    );
}
