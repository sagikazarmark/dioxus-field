use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use dioxus::prelude::*;
use dioxus_field::{Binding, Field, FieldContext, FieldMetaValues};

#[test]
fn context_equality_follows_binding_identity_and_metadata() {
    fn app() -> Element {
        let signal = use_signal(|| 1);
        let other_signal = use_signal(|| 1);
        let binding: Binding<i32> = signal.into();

        assert_eq!(
            FieldContext::new(binding.clone()),
            FieldContext::new(binding.clone()),
            "contexts erased on different renders should compare by binding identity"
        );
        assert_eq!(
            FieldContext::empty().with_binding(binding.clone()),
            FieldContext::new(binding.clone()),
            "with_binding should erase with the same comparator as new"
        );
        assert_ne!(
            FieldContext::new(binding.clone()),
            FieldContext::new(Binding::<i32>::from(other_signal)),
            "different bindings should stay unequal"
        );
        assert_ne!(
            FieldContext::new(binding.clone()),
            FieldContext::new(Binding::<String>::from(use_signal(String::new))),
            "bindings of different value types should stay unequal"
        );
        assert_ne!(
            FieldContext::new(binding.clone()),
            FieldContext::empty(),
            "a binding should not equal its absence"
        );
        assert_eq!(FieldContext::empty(), FieldContext::empty());
        assert_ne!(
            FieldContext::new(binding.clone()).with_meta_values(FieldMetaValues::default()),
            FieldContext::new(binding).with_meta_values(FieldMetaValues {
                required: true,
                ..FieldMetaValues::default()
            }),
            "metadata differences should stay unequal"
        );

        VNode::empty()
    }

    VirtualDom::new(app).rebuild_in_place();
}

#[test]
fn bindings_and_signals_convert_into_context() {
    fn app() -> Element {
        let signal = use_signal(|| 7);
        let binding: Binding<i32> = signal.into();

        let from_binding: FieldContext = binding.clone().into();
        assert_eq!(from_binding.resolve::<i32>(), binding);

        let from_signal: FieldContext = signal.into();
        assert_eq!(from_signal.resolve::<i32>(), binding);
        assert_eq!(from_signal, from_binding);

        VNode::empty()
    }

    VirtualDom::new(app).rebuild_in_place();
}

#[derive(Clone, Copy, PartialEq)]
enum ContextSource {
    Stable,
    Inline,
}

#[derive(Clone, Copy, PartialEq)]
enum ChildrenSource {
    Forwarded,
    Inline,
}

#[derive(PartialEq)]
struct MemoizationProbe {
    context_source: ContextSource,
    children_source: ChildrenSource,
    field_child_renders: Cell<usize>,
    wrapper_tick: RefCell<Option<Signal<u32>>>,
}

#[derive(Clone, Props)]
struct FieldChildProps {
    probe: Rc<MemoizationProbe>,
}

impl PartialEq for FieldChildProps {
    /// Never memoizes, so this child renders exactly when [`Field`] renders.
    fn eq(&self, _: &Self) -> bool {
        false
    }
}

#[allow(non_snake_case)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Dioxus components receive their generated properties by value"
)]
fn FieldChild(props: FieldChildProps) -> Element {
    props
        .probe
        .field_child_renders
        .set(props.probe.field_child_renders.get() + 1);

    VNode::empty()
}

#[derive(Clone, Props, PartialEq)]
struct WrapperProps {
    probe: Rc<MemoizationProbe>,
    children: Element,
}

#[allow(non_snake_case)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Dioxus components receive their generated properties by value"
)]
fn Wrapper(props: WrapperProps) -> Element {
    let tick = use_signal(|| 0_u32);
    let _ = tick();
    props.probe.wrapper_tick.borrow_mut().replace(tick);

    let value = use_signal(|| 0);
    let stable = use_hook(|| FieldContext::from(value));
    let context = match props.probe.context_source {
        ContextSource::Stable => stable,
        ContextSource::Inline => FieldContext::from(value),
    };

    match props.probe.children_source {
        ChildrenSource::Forwarded => rsx! {
            Field { context, children: props.children }
        },
        ChildrenSource::Inline => rsx! {
            Field { context,
                FieldChild { probe: Rc::clone(&props.probe) }
            }
        },
    }
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn memoization_app(probe: Rc<MemoizationProbe>) -> Element {
    rsx! {
        Wrapper { probe: Rc::clone(&probe),
            FieldChild { probe: Rc::clone(&probe) }
        }
    }
}

fn field_renders_after_wrapper_rerender(
    context_source: ContextSource,
    children_source: ChildrenSource,
) -> usize {
    let probe = Rc::new(MemoizationProbe {
        context_source,
        children_source,
        field_child_renders: Cell::new(0),
        wrapper_tick: RefCell::new(None),
    });
    let mut dom = VirtualDom::new_with_props(memoization_app, Rc::clone(&probe));
    dom.rebuild_in_place();
    assert_eq!(probe.field_child_renders.get(), 1);

    probe
        .wrapper_tick
        .borrow_mut()
        .as_mut()
        .expect("wrapper should expose its tick signal")
        .set(1);
    dom.render_immediate_to_vec();

    probe.field_child_renders.get() - 1
}

#[test]
fn field_memoizes_on_context_equality_when_children_are_forwarded() {
    assert_eq!(
        field_renders_after_wrapper_rerender(ContextSource::Stable, ChildrenSource::Forwarded),
        0
    );
    assert_eq!(
        field_renders_after_wrapper_rerender(ContextSource::Inline, ChildrenSource::Forwarded),
        0,
        "a context built inline from the same signal should compare equal across renders"
    );
}

#[test]
fn inline_children_rerender_the_field_regardless_of_context_equality() {
    assert_eq!(
        field_renders_after_wrapper_rerender(ContextSource::Stable, ChildrenSource::Inline),
        1
    );
}

#[test]
fn provide_field_context_keeps_its_focus_slot_across_renders() {
    struct SlotProbe {
        rerender: RefCell<Option<Signal<u32>>>,
        slots: RefCell<Vec<dioxus_field::FocusRequest>>,
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "VirtualDom entrypoints receive their root properties by value"
    )]
    fn app(probe: Rc<SlotProbe>) -> Element {
        let rerender = use_signal(|| 0_u32);
        let _ = rerender();
        probe.rerender.borrow_mut().replace(rerender);

        let value = use_signal(|| 0);
        let context = dioxus_field::provide_field_context(Binding::<i32>::from(value));
        probe.slots.borrow_mut().push(context.focus_request());

        VNode::empty()
    }

    let probe = Rc::new(SlotProbe {
        rerender: RefCell::new(None),
        slots: RefCell::new(Vec::new()),
    });
    let mut dom = VirtualDom::new_with_props(app, Rc::clone(&probe));
    dom.rebuild_in_place();
    probe
        .rerender
        .borrow_mut()
        .as_mut()
        .expect("app should expose its rerender signal")
        .set(1);
    dom.render_immediate_to_vec();

    let slots = probe.slots.borrow();
    assert_eq!(slots.len(), 2);
    assert_eq!(
        slots[0], slots[1],
        "the provided slot should survive a re-render"
    );
}
