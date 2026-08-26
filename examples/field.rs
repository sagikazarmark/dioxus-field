use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_core::VirtualDom;
use dioxus_field::{
    Binding, Field, FieldContext, FieldDescription, FieldError, FieldMetaValues, Label,
    use_field_meta_state,
};

fn app() -> Element {
    let mut name = use_signal(String::new);
    let binding: Binding<String> = name.into();
    let meta = use_field_meta_state(FieldMetaValues {
        id: Some(Rc::from("profile-name")),
        name: Some(Rc::from("name")),
        required: true,
        errors: vec![Rc::from("Enter a name")],
        ..FieldMetaValues::default()
    });

    rsx! {
        Field { context: FieldContext::new(binding).with_meta(meta),
            Label { "Name" }
            input {
                value: name,
                oninput: move |event| name.set(event.value()),
                ..meta.attributes(),
            }
            FieldDescription { id: "profile-name-help", "Shown on your profile" }
            FieldError { id: "profile-name-error" }
        }
    }
}

fn main() {
    let mut dom = VirtualDom::new(app);
    dom.rebuild_in_place();
    // The label, description, and error parts register their ids while they render, so the
    // control's `aria-labelledby`, `aria-describedby`, and `aria-errormessage` land on the render
    // that follows their first mount.
    dom.render_immediate_to_vec();

    println!("{}", dioxus_ssr::render(&dom));
}
