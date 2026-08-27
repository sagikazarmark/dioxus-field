//! Reusable assertions for widget registry conformance tests.
//!
//! Registry tests wire these probes into their real components, drive the component through its
//! normal interaction path, then call the corresponding assertion. The probes deliberately do not
//! prescribe a rendered element or DOM event because those details belong to each widget.
//!
//! # Conformance levels
//!
//! The convention has two levels, and the kit certifies each:
//!
//! - **Trio-conformant** (no dependency on this crate): the widget honors the `value` /
//!   `on_change` / `on_commit` prop trio plus attribute spread. Applicable tests:
//!   [`CommitOrderProbe`] and [`ChangeOriginProbe`] (trio-only widgets imply
//!   [`ChangeOrigin::User`]).
//! - **Field-aware**: the widget additionally resolves the Field Context. Applicable tests: the
//!   three resolution-precedence assertions, both [`FocusRoundTripProbe`] assertions, and
//!   [`assert_field_part_ids`].
//!
//! Bindings that support Focus Exit can additionally use [`FocusExitProbe`] and
//! [`FocusExitOrderProbe`]. These probes are optional: they do not add requirements to the
//! dependency-free prop trio or to existing Commit-only field-aware conformance. Widget-specific
//! logical-scope detection and deduplication remain the registry's responsibility.
//!
//! # Example
//!
//! The [runnable interaction-probe adapter] demonstrates how callbacks created during rendering
//! reach a registry-owned driver. The [complete conformance test] exercises all six required tests
//! and the optional Focus Exit tests against a minimal field-aware widget.
//!
//! [runnable interaction-probe adapter]: https://docs.rs/crate/dioxus-field/latest/source/examples/conformance.rs
//! [complete conformance test]: https://docs.rs/crate/dioxus-field/latest/source/tests/conformance.rs

use std::{
    cell::{Cell, RefCell},
    fmt::Debug,
    rc::Rc,
};

use dioxus_core::{Attribute, AttributeValue, Callback};
use dioxus_signals::ReadSignal;

use crate::{Binding, ChangeOrigin, FieldControlOptions, FieldMeta};

/// Records the relative order of a widget commit and its containing submit handler.
#[derive(Clone, Debug, Default)]
pub struct CommitOrderProbe {
    events: Rc<RefCell<Vec<CommitOrderEvent>>>,
}

impl CommitOrderProbe {
    /// Creates an empty commit-order probe.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the callback to wire to the widget's `on_commit` path.
    pub fn on_commit(&self) -> Callback<()> {
        let events = Rc::clone(&self.events);
        Callback::new(move |()| events.borrow_mut().push(CommitOrderEvent::Commit))
    }

    /// Returns the callback to invoke from the containing submit handler.
    pub fn on_submit(&self) -> Callback<()> {
        let events = Rc::clone(&self.events);
        Callback::new(move |()| events.borrow_mut().push(CommitOrderEvent::Submit))
    }

    /// Asserts that one commit was synchronously observed before one submit.
    ///
    /// # Panics
    ///
    /// Panics when either callback was omitted, repeated, or observed out of order.
    pub fn assert_commit_before_submit(&self) {
        assert_eq!(
            *self.events.borrow(),
            [CommitOrderEvent::Commit, CommitOrderEvent::Submit],
            "the widget must synchronously commit exactly once before submit handling runs"
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommitOrderEvent {
    Commit,
    Submit,
}

/// Records reports that focus left a widget's complete logical focus scope.
///
/// This probe is optional and does not change Commit-only conformance. Registry tests can use
/// [`FocusExitProbe::assert_no_focus_exit`] after moving focus between owned controls or popup
/// content to verify that the widget retains the complete logical scope.
#[derive(Clone, Debug, Default)]
pub struct FocusExitProbe {
    focus_exits: Rc<Cell<usize>>,
}

impl FocusExitProbe {
    /// Creates an empty Focus Exit probe.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the callback to supply through [`Binding::with_focus_exit`].
    pub fn on_focus_exit(&self) -> Callback<()> {
        let focus_exits = Rc::clone(&self.focus_exits);
        Callback::new(move |()| focus_exits.set(focus_exits.get() + 1))
    }

    /// Asserts that Focus Exit was reported exactly once.
    ///
    /// # Panics
    ///
    /// Panics when the callback was omitted or invoked more than once.
    pub fn assert_focus_exit_once(&self) {
        assert_eq!(
            self.focus_exits.get(),
            1,
            "focus leaving the widget's complete logical scope must be reported exactly once"
        );
    }

    /// Asserts that Focus Exit was not reported.
    ///
    /// Use this after internal focus movement or a Commit that leaves focus inside the widget.
    ///
    /// # Panics
    ///
    /// Panics when Focus Exit was reported.
    pub fn assert_no_focus_exit(&self) {
        assert_eq!(
            self.focus_exits.get(),
            0,
            "internal focus movement and Commit must not imply Focus Exit"
        );
    }
}

impl PartialEq for FocusExitProbe {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.focus_exits, &other.focus_exits)
    }
}

/// Records the relative order of binding writes, Commits, and Focus Exits.
///
/// This optional probe creates a [`Binding`] for a registry adapter to drive through its normal
/// interaction path. Its write callback records the event but deliberately owns no value state;
/// use [`ChangeOriginProbe`] separately when the test also needs to assert values and origins.
#[derive(Clone, Debug, Default)]
pub struct FocusExitOrderProbe {
    events: Rc<RefCell<Vec<FocusExitOrderEvent>>>,
}

impl FocusExitOrderProbe {
    /// Creates an empty Focus Exit order probe.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a binding that records writes, Commits, and Focus Exits in call order.
    pub fn binding<T: 'static>(&self, read: ReadSignal<T>) -> Binding<T> {
        let write_events = Rc::clone(&self.events);
        let commit_events = Rc::clone(&self.events);
        let focus_exit_events = Rc::clone(&self.events);

        Binding::new(
            read,
            Callback::new(move |_| {
                write_events.borrow_mut().push(FocusExitOrderEvent::Write);
            }),
            Callback::new(move |()| {
                commit_events.borrow_mut().push(FocusExitOrderEvent::Commit);
            }),
        )
        .with_focus_exit(Callback::new(move |()| {
            focus_exit_events
                .borrow_mut()
                .push(FocusExitOrderEvent::FocusExit);
        }))
    }

    /// Asserts that one Commit occurred without a Focus Exit.
    ///
    /// # Panics
    ///
    /// Panics when Commit was omitted or repeated, or any write or Focus Exit was observed.
    pub fn assert_commit_without_focus_exit(&self) {
        assert_eq!(
            *self.events.borrow(),
            [FocusExitOrderEvent::Commit],
            "Commit while focus remains in the widget must not imply Focus Exit"
        );
    }

    /// Asserts that one synchronous write and Commit were observed before one Focus Exit.
    ///
    /// # Panics
    ///
    /// Panics when an event was omitted, repeated, or observed out of order.
    pub fn assert_write_and_commit_before_focus_exit(&self) {
        assert_eq!(
            *self.events.borrow(),
            [
                FocusExitOrderEvent::Write,
                FocusExitOrderEvent::Commit,
                FocusExitOrderEvent::FocusExit,
            ],
            "Focus Exit must follow any synchronous widget write and Commit for the interaction"
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusExitOrderEvent {
    Write,
    Commit,
    FocusExit,
}

/// Records values written through a [`Binding`] together with their [`ChangeOrigin`].
#[derive(Debug)]
pub struct ChangeOriginProbe<T> {
    writes: Rc<RefCell<Vec<(T, ChangeOrigin)>>>,
}

impl<T> ChangeOriginProbe<T> {
    /// Creates an empty write probe.
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T: 'static> ChangeOriginProbe<T> {
    /// Creates a binding whose writes are recorded by this probe.
    pub fn binding(&self, read: ReadSignal<T>) -> Binding<T> {
        self.binding_with_commit(read, Callback::new(|()| {}))
    }

    /// Creates a binding whose writes are recorded and whose commits use `on_commit`.
    pub fn binding_with_commit(&self, read: ReadSignal<T>, on_commit: Callback<()>) -> Binding<T> {
        let writes = Rc::clone(&self.writes);

        Binding::new(
            read,
            Callback::new(move |write| writes.borrow_mut().push(write)),
            on_commit,
        )
    }
}

impl<T: Debug + PartialEq> ChangeOriginProbe<T> {
    /// Asserts the complete ordered sequence of value and origin pairs.
    ///
    /// # Panics
    ///
    /// Panics when the observed writes differ from `expected`.
    pub fn assert_writes(&self, expected: &[(T, ChangeOrigin)]) {
        assert_eq!(
            self.writes.borrow().as_slice(),
            expected,
            "widget writes must retain their change origin"
        );
    }
}

impl<T> Clone for ChangeOriginProbe<T> {
    fn clone(&self) -> Self {
        Self {
            writes: Rc::clone(&self.writes),
        }
    }
}

impl<T> Default for ChangeOriginProbe<T> {
    fn default() -> Self {
        Self {
            writes: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

/// The independently overridable metadata flags required by the convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverridableMetaFlags {
    /// The resolved invalid state.
    pub invalid: bool,
    /// The resolved disabled state.
    pub disabled: bool,
    /// The resolved required state.
    pub required: bool,
}

impl OverridableMetaFlags {
    /// Creates one observed or expected set of metadata flags, with `required` left false.
    pub const fn new(invalid: bool, disabled: bool) -> Self {
        Self {
            invalid,
            disabled,
            required: false,
        }
    }

    /// Returns these flags with the resolved required state replaced.
    #[must_use]
    pub const fn with_required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

/// Asserts explicit binding, context binding, then internal-state resolution precedence.
///
/// The registry adapter should expose the binding its widget resolved in the first two scenarios
/// and the value observed after writing its uncontrolled binding in the final scenario.
///
/// # Panics
///
/// Panics when either resolved binding has the wrong identity or internal state did not retain its
/// write.
#[allow(
    clippy::needless_pass_by_value,
    reason = "owned observed values keep the assertion API convenient for registry tests"
)]
pub fn assert_binding_resolution_precedence<T: Debug + PartialEq + 'static>(
    resolved_with_explicit: &Binding<T>,
    explicit: &Binding<T>,
    resolved_with_context: &Binding<T>,
    context: &Binding<T>,
    internal_value: T,
    expected_internal_value: T,
) {
    assert!(
        resolved_with_explicit == explicit,
        "an explicit binding must win over Field Context"
    );
    assert!(
        resolved_with_context == context,
        "Field Context must win when no explicit binding is present"
    );
    assert_eq!(
        internal_value, expected_internal_value,
        "internal state must be used when neither an explicit binding nor Field Context is present"
    );
}

/// Asserts explicit metadata, context metadata, then standalone metadata resolution precedence.
///
/// # Panics
///
/// Panics when either resolved metadata handle has the wrong identity or the standalone flags do
/// not match the expected defaults.
pub fn assert_meta_resolution_precedence(
    resolved_with_explicit: FieldMeta,
    explicit: FieldMeta,
    resolved_with_context: FieldMeta,
    context: FieldMeta,
    standalone_flags: OverridableMetaFlags,
    expected_standalone_flags: OverridableMetaFlags,
) {
    assert!(
        resolved_with_explicit == explicit,
        "explicit metadata must win over Field Context"
    );
    assert!(
        resolved_with_context == context,
        "Field Context metadata must win when explicit metadata is absent"
    );
    assert_eq!(
        standalone_flags, expected_standalone_flags,
        "standalone metadata must be used when neither explicit metadata nor Field Context is present"
    );
}

/// Asserts the invalid and disabled flags observed after applying explicit per-flag props.
///
/// Registry tests should obtain `observed` from the actual attributes or state rendered by their
/// widget, not by recomputing metadata resolution in the test.
///
/// # Panics
///
/// Panics when either observed flag differs from the expected explicit-or-metadata result.
pub fn assert_meta_flag_precedence(observed: OverridableMetaFlags, expected: OverridableMetaFlags) {
    assert_eq!(
        observed, expected,
        "each explicit metadata flag must override only its corresponding metadata flag"
    );
}

/// Records focus callbacks reached through a widget's resolved [`crate::FocusRequest`].
#[derive(Clone, Debug, Default)]
pub struct FocusRoundTripProbe {
    focus_calls: Rc<Cell<usize>>,
}

impl FocusRoundTripProbe {
    /// Creates an empty focus probe.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the callback the widget should register for its actual control.
    pub fn on_focus(&self) -> Callback<()> {
        let focus_calls = Rc::clone(&self.focus_calls);
        Callback::new(move |()| focus_calls.set(focus_calls.get() + 1))
    }

    /// Asserts that one producer focus request reached the widget control callback.
    ///
    /// # Panics
    ///
    /// Panics when the focus callback was omitted or invoked more than once.
    pub fn assert_focus_round_trip(&self) {
        assert_eq!(
            self.focus_calls.get(),
            1,
            "one producer focus request must reach the widget's control exactly once"
        );
    }

    /// Asserts that no producer focus request moved focus.
    ///
    /// Drive the widget's focus request while it is disabled, then call this. A disabled control
    /// must focus nothing: focusing a proxy element instead pulls focus off whatever the user was
    /// on, and `HTMLElement.focus()` reports success on a disabled element, so nothing downstream
    /// can detect the difference.
    ///
    /// # Panics
    ///
    /// Panics when the widget moved focus anyway.
    pub fn assert_focus_not_moved(&self) {
        assert_eq!(
            self.focus_calls.get(),
            0,
            "a focus request must not move focus while the control is disabled"
        );
    }
}

impl PartialEq for FocusRoundTripProbe {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.focus_calls, &other.focus_calls)
    }
}

/// Asserts the description and error ids currently registered in field metadata.
///
/// Call this once after the registry's description and error parts mount, then again with empty
/// expected slices after they drop. Id order must match mount order because ARIA id references are
/// rendered in registration order.
///
/// While the field is invalid, `aria-describedby` carries the description ids followed by every
/// error id, and `aria-errormessage` carries only the first error id — it takes a single IDREF in
/// ARIA 1.2, so a list there is malformed and exposes no error at all.
///
/// # Panics
///
/// Panics when the metadata's ARIA id references differ from the expected ids.
pub fn assert_field_part_ids(
    meta: FieldMeta,
    expected_description_ids: &[&str],
    expected_error_ids: &[&str],
) {
    let attributes = meta.attributes_for(&FieldControlOptions::new().invalid(Some(true)));
    let mut expected_described_by = expected_description_ids.to_vec();
    expected_described_by.extend_from_slice(expected_error_ids);

    assert_eq!(
        attribute_text(&attributes, "aria-describedby"),
        joined_ids(&expected_described_by),
        "description ids, then error ids, must match the currently mounted parts"
    );
    assert_eq!(
        attribute_text(&attributes, "aria-errormessage").as_deref(),
        expected_error_ids.first().copied(),
        "aria-errormessage must reference the first mounted error part and nothing else"
    );
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

fn joined_ids(ids: &[&str]) -> Option<String> {
    (!ids.is_empty()).then(|| ids.join(" "))
}
