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
        Field { binding: FieldContext::new(binding).with_meta(meta),
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

    println!("{}", dioxus_ssr::render(&dom));
}
