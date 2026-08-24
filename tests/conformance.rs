use std::{cell::RefCell, rc::Rc};

use dioxus::prelude::{Props, dioxus_elements, rsx};
use dioxus_core::{Attribute, Callback, Element, VNode, VirtualDom};
use dioxus_field::{
    Binding, ChangeOrigin, Field, FieldContext, FieldDescription, FieldError, FieldMeta,
    FieldMetaValues, FocusRequest,
    testing::{
        ChangeOriginProbe, CommitOrderProbe, FocusRoundTripProbe, OverridableMetaFlags,
        assert_binding_resolution_precedence, assert_field_part_ids, assert_meta_flag_precedence,
        assert_meta_resolution_precedence,
    },
    use_binding, use_field_meta, use_field_meta_state, use_focus_registration, use_focus_request,
};
use dioxus_hooks::use_signal;
use dioxus_signals::{ReadSignal, Signal, WritableExt};

#[derive(Default)]
struct WidgetDriverState {
    binding: Option<Binding<i32>>,
    meta: Option<FieldMeta>,
    flags: Option<OverridableMetaFlags>,
    on_change: Option<Callback<(i32, ChangeOrigin)>>,
    on_commit: Option<Callback<()>>,
}

#[derive(Clone, Default)]
struct WidgetDriver(Rc<RefCell<WidgetDriverState>>);

impl WidgetDriver {
    fn binding(&self) -> Binding<i32> {
        self.0
            .borrow()
            .binding
            .clone()
            .expect("widget should expose its resolved binding")
    }

    fn meta(&self) -> FieldMeta {
        self.0
            .borrow()
            .meta
            .expect("widget should expose its resolved metadata")
    }

    fn flags(&self) -> OverridableMetaFlags {
        self.0
            .borrow()
            .flags
            .expect("widget should expose its rendered metadata flags")
    }

    fn change(&self, value: i32, origin: ChangeOrigin) {
        self.0
            .borrow()
            .on_change
            .expect("widget should expose its change handler")
            .call((value, origin));
    }

    fn commit(&self) {
        self.0
            .borrow()
            .on_commit
            .expect("widget should expose its commit handler")
            .call(());
    }
}

impl PartialEq for WidgetDriver {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Props, PartialEq)]
struct ConformingWidgetProps {
    #[props(default)]
    binding: Option<Binding<i32>>,
    #[props(default)]
    meta: Option<FieldMeta>,
    #[props(default)]
    invalid: Option<bool>,
    #[props(default)]
    disabled: Option<bool>,
    #[props(default)]
    focus_probe: Option<FocusRoundTripProbe>,
    driver: WidgetDriver,
}

#[allow(non_snake_case)]
fn ConformingWidget(props: ConformingWidgetProps) -> Element {
    let binding = use_binding(props.binding, 30);
    let meta = use_field_meta(props.meta);
    let flags = OverridableMetaFlags::new(
        props.invalid.unwrap_or_else(|| meta.invalid()),
        props.disabled.unwrap_or_else(|| meta.disabled()),
    );
    let mut focused = use_signal(|| false);
    let on_control_focus = props.focus_probe.map(|probe| probe.on_focus());
    use_focus_registration(Callback::new(move |()| {
        focused.set(true);
        if let Some(on_control_focus) = on_control_focus {
            on_control_focus.call(());
        }
    }));

    let change_binding = binding.clone();
    let commit_binding = binding.clone();
    *props.driver.0.borrow_mut() = WidgetDriverState {
        binding: Some(binding.clone()),
        meta: Some(meta),
        flags: Some(flags),
        on_change: Some(Callback::new(move |(value, origin)| {
            change_binding.write(value, origin);
        })),
        on_commit: Some(Callback::new(move |()| commit_binding.commit())),
    };

    let mut attributes = Vec::new();

    if flags.invalid {
        attributes.push(Attribute::new("data-invalid", "true", None, false));
    }
    if flags.disabled {
        attributes.push(Attribute::new("data-disabled", "true", None, false));
    }
    if focused() {
        attributes.push(Attribute::new("data-focused", "true", None, false));
    }

    rsx! {
        input {
            value: (binding.read)(),
            ..attributes,
        }
    }
}

#[derive(Clone)]
struct InteractionHarness {
    changes: ChangeOriginProbe<i32>,
    commits: CommitOrderProbe,
    driver: WidgetDriver,
    on_submit: Rc<RefCell<Option<Callback<()>>>>,
}

fn interaction_app(harness: InteractionHarness) -> Element {
    let value = use_signal(|| 1);
    harness
        .on_submit
        .borrow_mut()
        .replace(harness.commits.on_submit());
    let binding = harness
        .changes
        .binding_with_commit(ReadSignal::from(value), harness.commits.on_commit());

    rsx! { ConformingWidget { binding, driver: harness.driver } }
}

#[test]
fn commit_is_synchronously_observable_before_submit_handling_runs() {
    let changes = ChangeOriginProbe::new();
    let commits = CommitOrderProbe::new();
    let driver = WidgetDriver::default();
    let on_submit = Rc::new(RefCell::new(None));
    let mut dom = VirtualDom::new_with_props(
        interaction_app,
        InteractionHarness {
            changes,
            commits: commits.clone(),
            driver: driver.clone(),
            on_submit: Rc::clone(&on_submit),
        },
    );
    dom.rebuild_in_place();

    dom.in_runtime(|| {
        driver.commit();
        on_submit
            .borrow()
            .expect("app should expose its submit handler")
            .call(());
    });

    commits.assert_commit_before_submit();
}

#[test]
fn writes_carry_their_change_origin() {
    let changes = ChangeOriginProbe::new();
    let commits = CommitOrderProbe::new();
    let driver = WidgetDriver::default();
    let mut dom = VirtualDom::new_with_props(
        interaction_app,
        InteractionHarness {
            changes: changes.clone(),
            commits,
            driver: driver.clone(),
            on_submit: Rc::new(RefCell::new(None)),
        },
    );
    dom.rebuild_in_place();

    dom.in_runtime(|| {
        driver.change(2, ChangeOrigin::User);
        driver.change(3, ChangeOrigin::Programmatic);
    });

    changes.assert_writes(&[(2, ChangeOrigin::User), (3, ChangeOrigin::Programmatic)]);
}

#[derive(Clone)]
struct ResolutionHarness {
    explicit_driver: WidgetDriver,
    context_driver: WidgetDriver,
    internal_driver: WidgetDriver,
    expected_bindings: ExpectedPair<Binding<i32>>,
    expected_meta: ExpectedPair<FieldMeta>,
}

type ExpectedPair<T> = Rc<RefCell<Option<(T, T)>>>;

fn resolution_app(harness: ResolutionHarness) -> Element {
    let context_signal = use_signal(|| 10);
    let explicit_signal = use_signal(|| 20);
    let context_binding: Binding<i32> = context_signal.into();
    let explicit_binding: Binding<i32> = explicit_signal.into();
    let context_meta = use_field_meta_state(FieldMetaValues {
        disabled: true,
        invalid: Some(true),
        ..FieldMetaValues::default()
    });
    let explicit_meta = use_field_meta_state(FieldMetaValues {
        invalid: Some(true),
        ..FieldMetaValues::default()
    });
    harness
        .expected_bindings
        .borrow_mut()
        .replace((explicit_binding.clone(), context_binding.clone()));
    harness
        .expected_meta
        .borrow_mut()
        .replace((explicit_meta, context_meta));

    rsx! {
        Field {
            context: FieldContext::new(context_binding).with_meta(context_meta),
            ConformingWidget {
                binding: explicit_binding,
                meta: explicit_meta,
                invalid: false,
                driver: harness.explicit_driver,
            }
            ConformingWidget {
                disabled: false,
                driver: harness.context_driver,
            }
        }
        ConformingWidget { driver: harness.internal_driver }
    }
}

#[test]
fn binding_resolution_precedence_holds_for_values_and_meta_flags() {
    let explicit_driver = WidgetDriver::default();
    let context_driver = WidgetDriver::default();
    let internal_driver = WidgetDriver::default();
    let expected_bindings = Rc::new(RefCell::new(None));
    let expected_meta = Rc::new(RefCell::new(None));
    let mut dom = VirtualDom::new_with_props(
        resolution_app,
        ResolutionHarness {
            explicit_driver: explicit_driver.clone(),
            context_driver: context_driver.clone(),
            internal_driver: internal_driver.clone(),
            expected_bindings: Rc::clone(&expected_bindings),
            expected_meta: Rc::clone(&expected_meta),
        },
    );
    dom.rebuild_in_place();
    dom.in_runtime(|| internal_driver.change(31, ChangeOrigin::User));

    let expected_bindings = expected_bindings.borrow();
    let (explicit_binding, context_binding) = expected_bindings
        .as_ref()
        .expect("app should expose expected bindings");
    assert_binding_resolution_precedence(
        &explicit_driver.binding(),
        explicit_binding,
        &context_driver.binding(),
        context_binding,
        dom.in_runtime(|| (internal_driver.binding().read)()),
        31,
    );

    let expected_meta = expected_meta.borrow();
    let (explicit_meta, context_meta) = expected_meta
        .as_ref()
        .expect("app should expose expected metadata");
    assert_meta_resolution_precedence(
        explicit_driver.meta(),
        *explicit_meta,
        context_driver.meta(),
        *context_meta,
        internal_driver.flags(),
        OverridableMetaFlags::new(false, false),
    );
    assert_meta_flag_precedence(
        explicit_driver.flags(),
        OverridableMetaFlags::new(false, false),
    );
    assert_meta_flag_precedence(
        context_driver.flags(),
        OverridableMetaFlags::new(true, false),
    );
}

#[derive(Clone, PartialEq)]
struct FocusHarness {
    probe: FocusRoundTripProbe,
    driver: WidgetDriver,
    request: Rc<RefCell<Option<FocusRequest>>>,
}

#[derive(Clone, Props, PartialEq)]
struct FocusRequesterProps {
    harness: FocusHarness,
}

#[allow(non_snake_case)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Dioxus components receive their generated properties by value"
)]
fn FocusRequester(props: FocusRequesterProps) -> Element {
    props
        .harness
        .request
        .borrow_mut()
        .replace(use_focus_request());
    VNode::empty()
}

fn focus_app(harness: FocusHarness) -> Element {
    rsx! {
        Field {
            context: FieldContext::empty(),
            ConformingWidget {
                focus_probe: harness.probe.clone(),
                driver: harness.driver.clone(),
            }
            FocusRequester { harness }
        }
    }
}

#[test]
fn focus_request_round_trips_to_the_widget_control() {
    let probe = FocusRoundTripProbe::new();
    let request = Rc::new(RefCell::new(None));
    let mut dom = VirtualDom::new_with_props(
        focus_app,
        FocusHarness {
            probe: probe.clone(),
            driver: WidgetDriver::default(),
            request: Rc::clone(&request),
        },
    );
    dom.rebuild_in_place();

    assert!(
        request
            .borrow()
            .as_ref()
            .expect("widget should expose a focus request")
            .request()
    );
    dom.render_immediate_to_vec();

    probe.assert_focus_round_trip();
    assert!(dioxus_ssr::render(&dom).contains("data-focused=\"true\""));
}

#[derive(Default)]
struct IdRegistrationHarness {
    meta: RefCell<Option<FieldMeta>>,
    show_parts: RefCell<Option<Signal<bool>>>,
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn id_registration_app(harness: Rc<IdRegistrationHarness>) -> Element {
    let meta = use_field_meta_state(FieldMetaValues::default());
    let show_parts = use_signal(|| true);
    harness.meta.borrow_mut().replace(meta);
    harness.show_parts.borrow_mut().replace(show_parts);

    rsx! {
        Field {
            context: FieldContext::empty().with_meta(meta),
            if show_parts() {
                FieldDescription { id: "registry-description", "Description" }
                FieldError { id: "registry-error" }
            }
        }
    }
}

#[test]
fn error_and_description_ids_appear_on_mount_and_vanish_on_drop() {
    let harness = Rc::new(IdRegistrationHarness::default());
    let mut dom = VirtualDom::new_with_props(id_registration_app, Rc::clone(&harness));
    dom.rebuild_in_place();
    let meta = harness.meta.borrow().expect("app should expose metadata");

    assert_field_part_ids(meta, &["registry-description"], &["registry-error"]);

    harness
        .show_parts
        .borrow_mut()
        .as_mut()
        .expect("app should expose part visibility")
        .set(false);
    dom.render_immediate_to_vec();
    assert_field_part_ids(meta, &[], &[]);
}
