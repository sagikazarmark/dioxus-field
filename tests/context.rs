use dioxus::prelude::*;
use dioxus_field::{Binding, FieldContext};

#[test]
fn bindings_and_signals_convert_into_context() {
    fn app() -> Element {
        let signal = use_signal(|| 7);
        let binding: Binding<i32> = signal.into();

        let from_binding: FieldContext = binding.clone().into();
        assert_eq!(from_binding.resolve::<i32>(), binding);

        let from_signal: FieldContext = signal.into();
        assert_eq!(from_signal.resolve::<i32>(), binding);

        VNode::empty()
    }

    VirtualDom::new(app).rebuild_in_place();
}
