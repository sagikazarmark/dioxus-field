use std::{cell::RefCell, rc::Rc};

use dioxus_core::{Callback, VNode, VirtualDom};
use dioxus_field::{
    Binding, BindingPropTrio, ChangeOrigin, FieldContext, provide_field_context, use_binding,
};
use dioxus_hooks::use_signal;
use dioxus_signals::{ReadSignal, Signal};

#[test]
fn from_signal_preserves_two_way_access_and_identity() {
    fn app() -> dioxus_core::Element {
        let signal = use_signal(|| 1);
        let first: Binding<i32> = signal.into();
        let second: Binding<i32> = signal.into();

        assert_eq!((first.read)(), 1);
        assert_eq!(first, second);

        first.write(2, ChangeOrigin::Programmatic);
        assert_eq!(signal(), 2);

        VNode::empty()
    }

    VirtualDom::new(app).rebuild_in_place();
}

#[test]
fn from_read_signal_and_callback_forwards_writes() {
    fn app(writes: Rc<RefCell<Vec<i32>>>) -> dioxus_core::Element {
        let signal = use_signal(|| 3);
        let write = Callback::new(move |value| writes.borrow_mut().push(value));
        let binding: Binding<i32> = (ReadSignal::from(signal), write).into();

        assert_eq!((binding.read)(), 3);
        binding.write(4, ChangeOrigin::User);

        VNode::empty()
    }

    let writes = Rc::new(RefCell::new(Vec::new()));
    VirtualDom::new_with_props(app, Rc::clone(&writes)).rebuild_in_place();

    assert_eq!(*writes.borrow(), [4]);
}

#[test]
fn from_plain_value_creates_an_uncontrolled_binding() {
    fn app() -> dioxus_core::Element {
        let binding: Binding<i32> = 5.into();

        assert_eq!((binding.read)(), 5);
        binding.write(6, ChangeOrigin::User);
        assert_eq!((binding.read)(), 6);

        VNode::empty()
    }

    VirtualDom::new(app).rebuild_in_place();
}

#[test]
fn trio_decomposition_marks_changes_as_user_changes_and_forwards_commit() {
    #[derive(Default)]
    struct Probe {
        writes: RefCell<Vec<(i32, ChangeOrigin)>>,
        commits: RefCell<usize>,
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "VirtualDom entrypoints receive their root properties by value"
    )]
    fn app(probe: Rc<Probe>) -> dioxus_core::Element {
        let signal = use_signal(|| 7);
        let write_probe = Rc::clone(&probe);
        let commit_probe = Rc::clone(&probe);
        let binding = Binding::new(
            ReadSignal::from(signal),
            Callback::new(move |write| write_probe.writes.borrow_mut().push(write)),
            Callback::new(move |()| *commit_probe.commits.borrow_mut() += 1),
        );
        binding.write(9, ChangeOrigin::Programmatic);
        let BindingPropTrio {
            value,
            on_change,
            on_commit,
        } = binding.into_trio();

        assert_eq!(value(), 7);
        on_change.call(8);
        on_commit.call(());

        VNode::empty()
    }

    let probe = Rc::new(Probe::default());
    VirtualDom::new_with_props(app, Rc::clone(&probe)).rebuild_in_place();

    assert_eq!(
        *probe.writes.borrow(),
        [(9, ChangeOrigin::Programmatic), (8, ChangeOrigin::User)]
    );
    assert_eq!(*probe.commits.borrow(), 1);
}

#[test]
fn focus_exit_is_optional_independent_and_part_of_binding_identity() {
    #[derive(Default)]
    struct Probe {
        commits: RefCell<usize>,
        focus_exits: RefCell<usize>,
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "VirtualDom entrypoints receive their root properties by value"
    )]
    fn app(probe: Rc<Probe>) -> dioxus_core::Element {
        let signal = use_signal(|| 7);
        let commit_probe = Rc::clone(&probe);
        let focus_exit_probe = Rc::clone(&probe);
        let commit = Callback::new(move |()| *commit_probe.commits.borrow_mut() += 1);
        let focus_exit = Callback::new(move |()| *focus_exit_probe.focus_exits.borrow_mut() += 1);
        let binding = Binding::new_with_identity(
            ReadSignal::from(signal),
            Callback::new(|_| {}),
            commit,
            "shared binding",
        )
        .with_focus_exit(focus_exit);
        let equal = Binding::new_with_identity(
            ReadSignal::from(signal),
            Callback::new(|_| {}),
            commit,
            "shared binding",
        )
        .with_focus_exit(focus_exit);
        let different_focus_exit = Binding::new_with_identity(
            ReadSignal::from(signal),
            Callback::new(|_| {}),
            commit,
            "shared binding",
        )
        .with_focus_exit(Callback::new(|()| {}));

        binding.commit();
        assert_eq!(*probe.focus_exits.borrow(), 0);
        binding.focus_exit();
        assert_eq!(*probe.commits.borrow(), 1);
        assert_eq!(*probe.focus_exits.borrow(), 1);
        assert_eq!(binding, equal);
        assert_ne!(binding, different_focus_exit);

        VNode::empty()
    }

    let probe = Rc::new(Probe::default());
    VirtualDom::new_with_props(app, Rc::clone(&probe)).rebuild_in_place();

    assert_eq!(*probe.commits.borrow(), 1);
    assert_eq!(*probe.focus_exits.borrow(), 1);
}

#[test]
fn existing_constructors_and_conversions_install_noop_focus_exit() {
    fn app() -> dioxus_core::Element {
        let signal = use_signal(|| 7);
        let read = ReadSignal::from(signal);
        let write = Callback::new(|_| {});
        let commit = Callback::new(|()| {});
        let on_change = Callback::new(|_| {});

        Binding::new(read, write, commit).focus_exit();
        Binding::new_with_identity(read, write, commit, "binding identity").focus_exit();

        let from_signal: Binding<i32> = signal.into();
        from_signal.focus_exit();
        let from_pair: Binding<i32> = (read, on_change).into();
        from_pair.focus_exit();
        let from_value: Binding<i32> = 7.into();
        from_value.focus_exit();

        VNode::empty()
    }

    VirtualDom::new(app).rebuild_in_place();
}

#[test]
fn binding_resolution_prefers_explicit_then_context_then_internal_state() {
    fn app() -> dioxus_core::Element {
        let context_signal = use_signal(|| 10);
        let explicit_signal = use_signal(|| 20);
        let context: Binding<i32> = context_signal.into();
        let explicit: Binding<i32> = explicit_signal.into();
        provide_field_context(context.clone());

        let resolved_explicit = use_binding(Some(explicit.clone()), 30);
        assert_eq!(resolved_explicit, explicit);

        let resolved_context = use_binding(None, 30);
        assert_eq!(resolved_context, context);

        VNode::empty()
    }

    fn internal_app() -> dioxus_core::Element {
        let resolved: Binding<i32> = use_binding(None, 30);
        assert_eq!((resolved.read)(), 30);
        resolved.write(31, ChangeOrigin::User);
        assert_eq!((resolved.read)(), 31);

        VNode::empty()
    }

    VirtualDom::new(app).rebuild_in_place();
    VirtualDom::new(internal_app).rebuild_in_place();
}

#[test]
#[should_panic(expected = "Field Context contains a binding for")]
fn wrong_context_value_type_is_a_loud_error() {
    #[allow(
        clippy::needless_pass_by_value,
        reason = "VirtualDom entrypoints receive their root properties by value"
    )]
    fn app(context: Rc<RefCell<Option<FieldContext>>>) -> dioxus_core::Element {
        let signal = use_signal(|| String::from("text"));
        let binding: Binding<String> = signal.into();
        context.borrow_mut().replace(FieldContext::new(binding));
        VNode::empty()
    }

    let context = Rc::new(RefCell::new(None));
    VirtualDom::new_with_props(app, Rc::clone(&context)).rebuild_in_place();
    let _: Binding<i32> = context
        .borrow()
        .as_ref()
        .expect("probe should contain field context")
        .resolve();
}

#[test]
fn identity_equality_never_equates_independent_bindings() {
    fn app() -> dioxus_core::Element {
        let first_signal: Signal<i32> = use_signal(|| 1);
        let second_signal: Signal<i32> = use_signal(|| 1);
        let first: Binding<i32> = first_signal.into();
        let first_clone = first.clone();
        let second: Binding<i32> = second_signal.into();

        assert_eq!(first, first_clone);
        assert_ne!(first, second);

        VNode::empty()
    }

    VirtualDom::new(app).rebuild_in_place();
}
