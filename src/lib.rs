//! A form-library-agnostic field convention for Dioxus.
//!
//! Use this crate to connect form-library-owned values and metadata to field-shaped widgets without
//! coupling the widget library to a form implementation. [`Binding`] is the upper-level reactive
//! contract. Widget registries that do not depend on this crate can instead accept separate `value`,
//! `on_change`, and `on_commit` props matching the lower-level [`BindingPropTrio`] contract.
//!
//! # Quick start
//!
//! ```rust
//! use dioxus::prelude::*;
//! use dioxus_field::Field;
//!
//! fn app() -> Element {
//!     let mut name = use_signal(String::new);
//!
//!     rsx! {
//!         Field { context: name,
//!             input {
//!                 value: name,
//!                 oninput: move |event| name.set(event.value()),
//!             }
//!         }
//!     }
//! }
//! ```

use std::{any::Any, cell::RefCell, fmt, rc::Rc};

use dioxus::prelude::{Props, dioxus_elements, rsx};
use dioxus_core::{Attribute, Callback, Element, provide_context, try_consume_context, use_hook};
use dioxus_hooks::{use_effect, use_reactive, use_signal};
use dioxus_signals::{ReadSignal, ReadableExt, Signal, WritableExt};

pub mod testing;

/// Initial presentation metadata for one field-shaped value.
///
/// `invalid: None` derives invalidity from whether `errors` is empty. Setting it to `Some` keeps
/// invalidity independently controlled by the metadata producer.
#[allow(
    clippy::struct_excessive_bools,
    reason = "these independent presentation flags have no invalid combinations"
)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldMetaValues {
    /// The rendered control's element id.
    pub id: Option<Rc<str>>,
    /// The rendered control's name.
    pub name: Option<Rc<str>>,
    /// Whether the field is required according to its producer.
    pub required: bool,
    /// Whether the field is disabled according to its producer.
    pub disabled: bool,
    /// An explicit invalid state, or `None` to derive it from `errors`.
    pub invalid: Option<bool>,
    /// Pre-rendered error text.
    pub errors: Vec<Rc<str>>,
    /// Whether the field is touched according to its producer.
    pub touched: bool,
    /// Whether the field is dirty according to its producer.
    pub dirty: bool,
}

/// Per-use overrides applied while deriving field attributes or rendering a field part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldMetaOverrides {
    /// Overrides the metadata's invalid state when present.
    pub invalid: Option<bool>,
    /// Overrides the metadata's disabled state when present.
    pub disabled: Option<bool>,
}

/// Signal-backed presentation metadata for one field-shaped value.
///
/// The flag meanings are producer-defined. This type does not track an initial value or classify
/// validity. Error strings are already formatted for display before they cross this boundary.
#[derive(Clone, Copy, PartialEq)]
pub struct FieldMeta {
    id: Signal<Option<Rc<str>>>,
    name: Signal<Option<Rc<str>>>,
    required: Signal<bool>,
    disabled: Signal<bool>,
    invalid: Signal<Option<bool>>,
    errors: Signal<Vec<Rc<str>>>,
    touched: Signal<bool>,
    dirty: Signal<bool>,
    registered_ids: Signal<RegisteredIds>,
}

impl fmt::Debug for FieldMeta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldMeta")
            .field("id", &*self.id.peek())
            .field("name", &*self.name.peek())
            .field("required", &*self.required.peek())
            .field("disabled", &*self.disabled.peek())
            .field("invalid", &*self.invalid.peek())
            .field("errors", &*self.errors.peek())
            .field("touched", &*self.touched.peek())
            .field("dirty", &*self.dirty.peek())
            .finish_non_exhaustive()
    }
}

impl FieldMeta {
    /// Returns the rendered control id.
    pub fn id(&self) -> Option<Rc<str>> {
        (self.id)()
    }

    /// Replaces the rendered control id.
    pub fn set_id(&mut self, id: Option<Rc<str>>) {
        self.id.set(id);
    }

    /// Returns the rendered control name.
    pub fn name(&self) -> Option<Rc<str>> {
        (self.name)()
    }

    /// Replaces the rendered control name.
    pub fn set_name(&mut self, name: Option<Rc<str>>) {
        self.name.set(name);
    }

    /// Returns whether the field is required.
    pub fn required(&self) -> bool {
        (self.required)()
    }

    /// Replaces the producer-defined required state.
    pub fn set_required(&mut self, required: bool) {
        self.required.set(required);
    }

    /// Returns whether the field is disabled.
    pub fn disabled(&self) -> bool {
        (self.disabled)()
    }

    /// Replaces the producer-defined disabled state.
    pub fn set_disabled(&mut self, disabled: bool) {
        self.disabled.set(disabled);
    }

    /// Returns whether the field is invalid.
    ///
    /// An explicit invalid state wins; otherwise invalidity is derived from whether errors exist.
    pub fn invalid(&self) -> bool {
        (self.invalid)().unwrap_or_else(|| !(self.errors)().is_empty())
    }

    /// Sets an explicit invalid state, or restores error-derived invalidity with `None`.
    pub fn set_invalid(&mut self, invalid: Option<bool>) {
        self.invalid.set(invalid);
    }

    /// Returns the pre-rendered error text.
    pub fn errors(&self) -> Vec<Rc<str>> {
        (self.errors)()
    }

    /// Replaces the pre-rendered error text.
    pub fn set_errors(&mut self, errors: Vec<Rc<str>>) {
        self.errors.set(errors);
    }

    /// Returns whether the field is touched.
    pub fn touched(&self) -> bool {
        (self.touched)()
    }

    /// Replaces the producer-defined touched state.
    pub fn set_touched(&mut self, touched: bool) {
        self.touched.set(touched);
    }

    /// Returns whether the field is dirty.
    pub fn dirty(&self) -> bool {
        (self.dirty)()
    }

    /// Replaces the producer-defined dirty state.
    pub fn set_dirty(&mut self, dirty: bool) {
        self.dirty.set(dirty);
    }

    /// Registers a description element id until the returned registration is dropped.
    #[must_use]
    pub fn register_description_id(&mut self, id: Rc<str>) -> FieldMetaIdRegistration {
        self.register_id(RegisteredIdKind::Description, id)
    }

    /// Registers an error element id until the returned registration is dropped.
    #[must_use]
    pub fn register_error_id(&mut self, id: Rc<str>) -> FieldMetaIdRegistration {
        self.register_id(RegisteredIdKind::Error, id)
    }

    /// Returns attributes for a rendered control using the metadata's flag states.
    pub fn attributes(&self) -> Vec<Attribute> {
        self.attributes_with(FieldMetaOverrides::default())
    }

    /// Returns attributes for a rendered control, applying per-flag explicit overrides.
    pub fn attributes_with(&self, overrides: FieldMetaOverrides) -> Vec<Attribute> {
        let invalid = overrides.invalid.unwrap_or_else(|| self.invalid());
        let disabled = overrides.disabled.unwrap_or_else(|| self.disabled());
        let registered_ids = (self.registered_ids)();
        let description_ids = registered_ids.joined(RegisteredIdKind::Description);
        let error_ids = registered_ids.joined(RegisteredIdKind::Error);
        let mut attributes = Vec::new();

        push_optional_text(&mut attributes, "id", self.id());
        push_optional_text(&mut attributes, "name", self.name());
        push_bool(&mut attributes, "required", self.required());
        push_bool(&mut attributes, "disabled", disabled);
        attributes.push(Attribute::new(
            "aria-invalid",
            invalid.to_string(),
            None,
            false,
        ));
        push_optional_text(&mut attributes, "aria-describedby", description_ids);
        if invalid {
            push_optional_text(&mut attributes, "aria-errormessage", error_ids);
        }
        push_data_state(&mut attributes, "data-required", self.required());
        push_data_state(&mut attributes, "data-disabled", disabled);
        push_data_state(&mut attributes, "data-invalid", invalid);
        push_data_state(&mut attributes, "data-touched", self.touched());
        push_data_state(&mut attributes, "data-dirty", self.dirty());

        attributes
    }

    fn register_id(&mut self, kind: RegisteredIdKind, id: Rc<str>) -> FieldMetaIdRegistration {
        let token = self.registered_ids.with_mut(|ids| ids.insert(kind, id));

        FieldMetaIdRegistration {
            registered_ids: self.registered_ids,
            token,
        }
    }

    /// Replaces producer-owned metadata values without disturbing registered part ids.
    pub fn set_values(&mut self, values: FieldMetaValues) {
        if *self.id.peek() != values.id {
            self.id.set(values.id);
        }
        if *self.name.peek() != values.name {
            self.name.set(values.name);
        }
        if *self.required.peek() != values.required {
            self.required.set(values.required);
        }
        if *self.disabled.peek() != values.disabled {
            self.disabled.set(values.disabled);
        }
        if *self.invalid.peek() != values.invalid {
            self.invalid.set(values.invalid);
        }
        if *self.errors.peek() != values.errors {
            self.errors.set(values.errors);
        }
        if *self.touched.peek() != values.touched {
            self.touched.set(values.touched);
        }
        if *self.dirty.peek() != values.dirty {
            self.dirty.set(values.dirty);
        }
    }
}

/// Creates signal-backed field metadata owned by the current component scope.
pub fn use_field_meta_state(initial: FieldMetaValues) -> FieldMeta {
    let FieldMetaValues {
        id,
        name,
        required,
        disabled,
        invalid,
        errors,
        touched,
        dirty,
    } = initial;

    FieldMeta {
        id: use_signal(|| id),
        name: use_signal(|| name),
        required: use_signal(|| required),
        disabled: use_signal(|| disabled),
        invalid: use_signal(|| invalid),
        errors: use_signal(|| errors),
        touched: use_signal(|| touched),
        dirty: use_signal(|| dirty),
        registered_ids: use_signal(RegisteredIds::default),
    }
}

fn use_synced_field_meta_state(values: &FieldMetaValues) -> FieldMeta {
    let meta = use_field_meta_state(values.clone());
    use_effect(use_reactive(values, move |values| {
        let mut meta = meta;
        meta.set_values(values);
    }));

    meta
}

/// A lifecycle-bound description or error id registration.
pub struct FieldMetaIdRegistration {
    registered_ids: Signal<RegisteredIds>,
    token: u64,
}

impl fmt::Debug for FieldMetaIdRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldMetaIdRegistration")
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

impl Drop for FieldMetaIdRegistration {
    fn drop(&mut self) {
        self.registered_ids
            .with_mut(|ids| ids.entries.retain(|entry| entry.token != self.token));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegisteredIdKind {
    Description,
    Error,
}

#[derive(Clone, Default, PartialEq, Eq)]
struct RegisteredIds {
    next_token: u64,
    entries: Vec<RegisteredId>,
}

impl RegisteredIds {
    fn insert(&mut self, kind: RegisteredIdKind, id: Rc<str>) -> u64 {
        let token = self.next_token;
        self.next_token += 1;
        self.entries.push(RegisteredId { token, kind, id });

        token
    }

    fn joined(&self, kind: RegisteredIdKind) -> Option<Rc<str>> {
        let ids = self
            .entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| entry.id.as_ref())
            .collect::<Vec<_>>();

        (!ids.is_empty()).then(|| Rc::from(ids.join(" ")))
    }
}

#[derive(Clone, PartialEq, Eq)]
struct RegisteredId {
    token: u64,
    kind: RegisteredIdKind,
    id: Rc<str>,
}

fn push_optional_text(attributes: &mut Vec<Attribute>, name: &'static str, value: Option<Rc<str>>) {
    if let Some(value) = value {
        attributes.push(Attribute::new(name, value.to_string(), None, false));
    }
}

fn push_bool(attributes: &mut Vec<Attribute>, name: &'static str, value: bool) {
    if value {
        attributes.push(Attribute::new(name, true, None, false));
    }
}

fn push_data_state(attributes: &mut Vec<Attribute>, name: &'static str, value: bool) {
    if value {
        attributes.push(Attribute::new(name, "true", None, false));
    }
}

/// Describes whether a value write came from user interaction or application code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeOrigin {
    /// The user changed the value through a widget.
    User,
    /// Application code changed the value.
    Programmatic,
}

/// A reactive, two-way binding to one field-shaped value.
///
/// Equality compares the binding's producer-defined identity. Equal bindings are guaranteed to
/// represent the same read and write behavior; producers may conservatively return unequal
/// bindings when they cannot prove that interchangeability.
pub struct Binding<T: 'static> {
    /// The binding's reactive value.
    pub read: ReadSignal<T>,
    write: Callback<(T, ChangeOrigin)>,
    commit: Callback<()>,
    identity: BindingIdentity,
}

impl<T: 'static> Binding<T> {
    /// Creates a binding identified by its exact read, write, and commit handles.
    pub fn new(
        read: ReadSignal<T>,
        write: Callback<(T, ChangeOrigin)>,
        commit: Callback<()>,
    ) -> Self {
        Self::new_with_identity(read, write, commit, (read, write, commit))
    }

    /// Creates a binding with a producer-defined comparable identity.
    ///
    /// Equal identities must always represent interchangeable read, write, and commit behavior.
    /// Producers that cannot prove interchangeability should use [`Binding::new`] instead.
    pub fn new_with_identity<I>(
        read: ReadSignal<T>,
        write: Callback<(T, ChangeOrigin)>,
        commit: Callback<()>,
        identity: I,
    ) -> Self
    where
        I: PartialEq + 'static,
    {
        Self {
            read,
            write,
            commit,
            identity: BindingIdentity::new(identity),
        }
    }

    /// Writes a value and preserves where the change originated.
    pub fn write(&self, value: T, origin: ChangeOrigin) {
        self.write.call((value, origin));
    }

    /// Reports the widget-defined end of one interaction unit.
    pub fn commit(&self) {
        self.commit.call(());
    }

    /// Decomposes this binding into the dependency-free widget prop contract.
    ///
    /// The lower-level `on_change` callback has no origin parameter, so its writes are user writes.
    pub fn into_trio(self) -> BindingPropTrio<T> {
        let value = self.read;
        let on_commit = self.commit;
        let on_change = Callback::new(move |value| self.write(value, ChangeOrigin::User));

        BindingPropTrio {
            value,
            on_change,
            on_commit,
        }
    }
}

impl<T: fmt::Debug + 'static> fmt::Debug for Binding<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Binding")
            .field("read", &*self.read.peek())
            .finish_non_exhaustive()
    }
}

impl<T: 'static> Clone for Binding<T> {
    fn clone(&self) -> Self {
        Self {
            read: self.read,
            write: self.write,
            commit: self.commit,
            identity: self.identity.clone(),
        }
    }
}

impl<T: 'static> PartialEq for Binding<T> {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
}

impl<T: 'static> From<Signal<T>> for Binding<T> {
    fn from(signal: Signal<T>) -> Self {
        let read = ReadSignal::from(signal);
        let mut writer = signal;
        let write = Callback::new(move |(value, _origin)| writer.set(value));
        let commit = Callback::new(|()| {});

        Self::new_with_identity(read, write, commit, signal)
    }
}

impl<T: 'static> From<(ReadSignal<T>, Callback<T>)> for Binding<T> {
    fn from((read, on_change): (ReadSignal<T>, Callback<T>)) -> Self {
        let write = Callback::new(move |(value, _origin)| on_change.call(value));
        let commit = Callback::new(|()| {});

        Self::new_with_identity(read, write, commit, (read, on_change))
    }
}

impl<T: 'static> From<T> for Binding<T> {
    fn from(value: T) -> Self {
        Signal::new(value).into()
    }
}

/// A carrier for the lower-level prop contract implemented by field-shaped widgets.
///
/// Decompose this carrier into three separate props to keep a widget independent from this crate.
/// Since `on_change` does not carry a [`ChangeOrigin`], calling it represents a user change.
pub struct BindingPropTrio<T: 'static> {
    /// The reactive value read by the widget.
    pub value: ReadSignal<T>,
    /// The callback invoked when user interaction changes the value.
    pub on_change: Callback<T>,
    /// The callback invoked at the widget-defined end of an interaction unit.
    pub on_commit: Callback<()>,
}

impl<T: fmt::Debug + 'static> fmt::Debug for BindingPropTrio<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BindingPropTrio")
            .field("value", &*self.value.peek())
            .field("on_change", &self.on_change)
            .field("on_commit", &self.on_commit)
            .finish()
    }
}

impl<T: 'static> From<Binding<T>> for BindingPropTrio<T> {
    fn from(binding: Binding<T>) -> Self {
        binding.into_trio()
    }
}

/// A field-scoped slot through which a widget exposes its focus behavior.
#[derive(Clone, Default)]
pub struct FocusRequest(Rc<RefCell<FocusRequestState>>);

impl FocusRequest {
    /// Registers the callback used by [`FocusRequest::request`].
    ///
    /// Dropping the returned registration removes this callback without disturbing a newer
    /// registration in the same slot.
    #[must_use]
    pub fn register(&self, callback: Callback<()>) -> FocusRegistration {
        let mut state = self.0.borrow_mut();
        let token = state.next_token;
        state.next_token += 1;
        state.current = Some((token, callback));

        FocusRegistration {
            request: self.clone(),
            token,
        }
    }

    /// Requests focus from the currently registered widget.
    ///
    /// Returns whether a widget was registered to receive the request.
    pub fn request(&self) -> bool {
        let callback = self.0.borrow().current.map(|(_, callback)| callback);

        if let Some(callback) = callback {
            callback.call(());
            true
        } else {
            false
        }
    }
}

impl fmt::Debug for FocusRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FocusRequest")
            .field("registered", &self.0.borrow().current.is_some())
            .finish()
    }
}

impl PartialEq for FocusRequest {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Default)]
struct FocusRequestState {
    next_token: u64,
    current: Option<(u64, Callback<()>)>,
}

/// A lifecycle-bound focus callback registration.
pub struct FocusRegistration {
    request: FocusRequest,
    token: u64,
}

impl fmt::Debug for FocusRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FocusRegistration")
            .field("request", &self.request)
            .field("token", &self.token)
            .finish()
    }
}

impl Drop for FocusRegistration {
    fn drop(&mut self) {
        let mut state = self.request.0.borrow_mut();

        if state.current.is_some_and(|(token, _)| token == self.token) {
            state.current = None;
        }
    }
}

/// Type-erased context for one field's binding, metadata, and focus request slot.
///
/// The context itself is intentionally not generic. This lets [`use_binding`] distinguish an
/// absent context from a present context containing the wrong value type.
#[derive(Clone)]
pub struct FieldContext {
    binding: Option<Rc<dyn Any>>,
    value_type_name: Option<&'static str>,
    meta: Option<FieldMeta>,
    meta_values: Option<FieldMetaValues>,
    focus_request: FocusRequest,
}

impl FieldContext {
    /// Creates context for a binding.
    pub fn new<T: 'static>(binding: Binding<T>) -> Self {
        Self {
            binding: Some(Rc::new(binding)),
            value_type_name: Some(std::any::type_name::<T>()),
            meta: None,
            meta_values: None,
            focus_request: FocusRequest::default(),
        }
    }

    /// Creates context with no value binding or metadata.
    pub fn empty() -> Self {
        Self {
            binding: None,
            value_type_name: None,
            meta: None,
            meta_values: None,
            focus_request: FocusRequest::default(),
        }
    }

    /// Replaces the context's value binding.
    #[must_use]
    pub fn with_binding<T: 'static>(mut self, binding: Binding<T>) -> Self {
        self.binding = Some(Rc::new(binding));
        self.value_type_name = Some(std::any::type_name::<T>());
        self
    }

    /// Adds signal-backed metadata to the context.
    #[must_use]
    pub fn with_meta(mut self, meta: FieldMeta) -> Self {
        self.meta = Some(meta);
        self.meta_values = None;
        self
    }

    /// Adds producer values that [`Field`] realizes as signal-backed metadata.
    #[must_use]
    pub fn with_meta_values(mut self, values: FieldMetaValues) -> Self {
        self.meta = None;
        self.meta_values = Some(values);
        self
    }

    /// Returns the context's metadata, when present.
    pub fn meta(&self) -> Option<FieldMeta> {
        self.meta
    }

    /// Returns the context's focus request slot.
    pub fn focus_request(&self) -> FocusRequest {
        self.focus_request.clone()
    }

    /// Resolves the context binding for `T`.
    ///
    /// # Panics
    ///
    /// Panics when the field context contains no binding or a binding for a different value type.
    pub fn resolve<T: 'static>(&self) -> Binding<T> {
        self.binding
            .as_ref()
            .unwrap_or_else(|| panic!("Field Context contains no value binding"))
            .downcast_ref::<Binding<T>>()
            .unwrap_or_else(|| {
                panic!(
                    "Field Context contains a binding for {}, but a binding for {} was requested",
                    self.value_type_name
                        .expect("a present binding should have a value type name"),
                    std::any::type_name::<T>()
                )
            })
            .clone()
    }

    fn try_resolve<T: 'static>(&self) -> Option<Binding<T>> {
        self.binding.as_ref().map(|_| self.resolve())
    }

    fn with_focus_request(mut self, focus_request: FocusRequest) -> Self {
        self.focus_request = focus_request;
        self
    }
}

impl fmt::Debug for FieldContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FieldContext")
            .field("value_type_name", &self.value_type_name)
            .field("meta", &self.meta)
            .field("meta_values", &self.meta_values)
            .field("focus_request", &self.focus_request)
            .finish_non_exhaustive()
    }
}

impl PartialEq for FieldContext {
    fn eq(&self, other: &Self) -> bool {
        match (&self.binding, &other.binding) {
            (Some(left), Some(right)) if !Rc::ptr_eq(left, right) => return false,
            (Some(_), None) | (None, Some(_)) => return false,
            _ => {}
        }

        self.meta == other.meta
            && self.meta_values == other.meta_values
            && self.focus_request == other.focus_request
    }
}

impl<T: 'static> From<Binding<T>> for FieldContext {
    fn from(binding: Binding<T>) -> Self {
        Self::new(binding)
    }
}

impl<T: 'static> From<Signal<T>> for FieldContext {
    fn from(signal: Signal<T>) -> Self {
        Self::new(Binding::<T>::from(signal))
    }
}

/// Provides a binding as the current scope's [`FieldContext`].
pub fn provide_field_context<T: 'static>(binding: Binding<T>) -> FieldContext {
    provide_context(FieldContext::new(binding))
}

/// Resolves a binding using explicit prop, [`FieldContext`], then uncontrolled-state precedence.
///
/// The internal signal hook is called regardless of which source wins so the resolution order can
/// change between renders without violating Dioxus's hook ordering rules.
pub fn use_binding<T: 'static>(explicit: Option<Binding<T>>, default: T) -> Binding<T> {
    let internal = use_signal(|| default);

    if let Some(binding) = explicit {
        return binding;
    }

    if let Some(binding) =
        try_consume_context::<FieldContext>().and_then(|context| context.try_resolve())
    {
        return binding;
    }

    internal.into()
}

/// Resolves metadata using explicit prop, [`FieldContext`], then standalone-state precedence.
///
/// The standalone state hook is always called so the source can change between renders without
/// violating Dioxus's hook ordering rules.
pub fn use_field_meta(explicit: Option<FieldMeta>) -> FieldMeta {
    let internal = use_field_meta_state(FieldMetaValues::default());

    explicit
        .or_else(|| try_consume_context::<FieldContext>().and_then(|context| context.meta()))
        .unwrap_or(internal)
}

/// Resolves the current [`FocusRequest`], or creates a standalone slot when no context exists.
pub fn use_focus_request() -> FocusRequest {
    let internal = use_hook(FocusRequest::default);

    try_consume_context::<FieldContext>().map_or(internal, |context| context.focus_request())
}

/// Registers a widget focus callback with the resolved [`FocusRequest`] for this component's
/// lifetime.
pub fn use_focus_registration(callback: Callback<()>) -> FocusRequest {
    let request = use_focus_request();
    let active = use_hook(|| Rc::new(RefCell::new(None::<ActiveFocusRegistration>)));
    let should_replace = active
        .borrow()
        .as_ref()
        .is_none_or(|active| active.request != request || active.callback != callback);

    if should_replace {
        let registration = request.register(callback);
        active.borrow_mut().replace(ActiveFocusRegistration {
            request: request.clone(),
            callback,
            _registration: registration,
        });
    }

    request
}

struct ActiveFocusRegistration {
    request: FocusRequest,
    callback: Callback<()>,
    _registration: FocusRegistration,
}

/// Props for the headless [`Field`] context provider.
#[derive(Clone, Debug, Props, PartialEq)]
pub struct FieldProps {
    /// The [`FieldContext`] provided to descendants.
    ///
    /// Accepts a [`FieldContext`], a [`Binding`], or a [`Signal`]. The prop is named after its
    /// payload rather than the binding it may carry, since a context can also hold only metadata.
    #[props(into)]
    pub context: FieldContext,
    /// Attributes forwarded to the rendered `div`.
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
    /// Field content.
    pub children: Element,
}

/// Provides one [`FieldContext`] and renders an unstyled `div` around its children.
///
/// On Dioxus 0.7.10, pass listeners through an explicit `attributes: vec![...]` prop so listener
/// ordering remains visible at the call site.
#[allow(non_snake_case)]
#[allow(
    clippy::missing_errors_doc,
    reason = "Dioxus Element uses Result as its renderer protocol"
)]
pub fn Field(props: FieldProps) -> Element {
    let has_meta_values = props.context.meta_values.is_some();
    let meta_values = props.context.meta_values.clone().unwrap_or_default();
    let synced_meta = use_synced_field_meta_state(&meta_values);
    let mut context = props.context;

    if has_meta_values {
        context = context.with_meta(synced_meta);
    }

    let focus_request = use_hook(|| context.focus_request());
    provide_context(context.with_focus_request(focus_request));

    rsx! {
        div { ..props.attributes, {props.children} }
    }
}

/// Props for the headless [`Label`] part.
#[derive(Clone, Debug, Props, PartialEq)]
pub struct LabelProps {
    /// Explicit metadata, which wins over Field Context metadata.
    #[props(default)]
    pub meta: Option<FieldMeta>,
    /// Explicit invalid state, which wins over the metadata state.
    #[props(default)]
    pub invalid: Option<bool>,
    /// Explicit disabled state, which wins over the metadata state.
    #[props(default)]
    pub disabled: Option<bool>,
    /// Attributes forwarded to the rendered `label`.
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
    /// Label content.
    pub children: Element,
}

/// Renders an unstyled `label` associated with the resolved metadata's control id.
///
/// This part can resolve metadata from Field Context, accept it explicitly, or run standalone.
#[allow(non_snake_case)]
#[allow(
    clippy::missing_errors_doc,
    reason = "Dioxus Element uses Result as its renderer protocol"
)]
pub fn Label(props: LabelProps) -> Element {
    let meta = use_field_meta(props.meta);
    let control_id = meta.id().map(|id| id.to_string());
    let mut attributes = part_state_attributes(
        &meta,
        FieldMetaOverrides {
            invalid: props.invalid,
            disabled: props.disabled,
        },
    );
    attributes.extend(props.attributes);

    rsx! {
        label { r#for: control_id, ..attributes, {props.children} }
    }
}

/// Props for the headless [`FieldDescription`] part.
#[derive(Clone, Debug, Props, PartialEq)]
pub struct FieldDescriptionProps {
    /// Stable id registered with the resolved field metadata for this part's lifetime.
    #[props(into)]
    pub id: Rc<str>,
    /// Explicit metadata, which wins over Field Context metadata.
    #[props(default)]
    pub meta: Option<FieldMeta>,
    /// Explicit invalid state, which wins over the metadata state.
    #[props(default)]
    pub invalid: Option<bool>,
    /// Explicit disabled state, which wins over the metadata state.
    #[props(default)]
    pub disabled: Option<bool>,
    /// Attributes forwarded to the rendered description `div`.
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
    /// Description content.
    pub children: Element,
}

/// Renders an unstyled description and registers its id for `aria-describedby` chaining.
///
/// This part can resolve metadata from Field Context, accept it explicitly, or run standalone.
#[allow(non_snake_case)]
#[allow(
    clippy::missing_errors_doc,
    reason = "Dioxus Element uses Result as its renderer protocol"
)]
pub fn FieldDescription(props: FieldDescriptionProps) -> Element {
    let meta = use_field_meta(props.meta);
    use_field_meta_id_registration(&meta, RegisteredIdKind::Description, props.id.clone());
    let id = props.id.to_string();
    let mut attributes = part_state_attributes(
        &meta,
        FieldMetaOverrides {
            invalid: props.invalid,
            disabled: props.disabled,
        },
    );
    attributes.extend(props.attributes);

    rsx! {
        div { id: id, ..attributes, {props.children} }
    }
}

/// Props for the headless [`FieldError`] part.
#[derive(Clone, Debug, Props, PartialEq)]
pub struct FieldErrorProps {
    /// Stable id registered with the resolved field metadata for this part's lifetime.
    #[props(into)]
    pub id: Rc<str>,
    /// Explicit metadata, which wins over Field Context metadata.
    #[props(default)]
    pub meta: Option<FieldMeta>,
    /// Explicit invalid state, which wins over the metadata state.
    #[props(default)]
    pub invalid: Option<bool>,
    /// Explicit disabled state used by data-state attributes.
    #[props(default)]
    pub disabled: Option<bool>,
    /// Attributes forwarded to the rendered error `div`.
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
}

/// Renders pre-formatted field errors in an unstyled polite live region while invalid.
///
/// This part can resolve metadata from Field Context, accept it explicitly, or run standalone.
#[allow(non_snake_case)]
#[allow(
    clippy::missing_errors_doc,
    reason = "Dioxus Element uses Result as its renderer protocol"
)]
pub fn FieldError(props: FieldErrorProps) -> Element {
    let meta = use_field_meta(props.meta);
    use_field_meta_id_registration(&meta, RegisteredIdKind::Error, props.id.clone());
    let invalid = props.invalid.unwrap_or_else(|| meta.invalid());

    if !invalid {
        return dioxus_core::VNode::empty();
    }

    let id = props.id.to_string();
    let errors = meta
        .errors()
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join("\n");
    let mut attributes = part_state_attributes(
        &meta,
        FieldMetaOverrides {
            invalid: props.invalid,
            disabled: props.disabled,
        },
    );
    attributes.extend(props.attributes);

    rsx! {
        div {
            id: id,
            aria_live: "polite",
            ..attributes,
            {errors}
        }
    }
}

fn part_state_attributes(meta: &FieldMeta, overrides: FieldMetaOverrides) -> Vec<Attribute> {
    let invalid = overrides.invalid.unwrap_or_else(|| meta.invalid());
    let disabled = overrides.disabled.unwrap_or_else(|| meta.disabled());
    let mut attributes = Vec::new();

    push_data_state(&mut attributes, "data-required", meta.required());
    push_data_state(&mut attributes, "data-disabled", disabled);
    push_data_state(&mut attributes, "data-invalid", invalid);
    push_data_state(&mut attributes, "data-touched", meta.touched());
    push_data_state(&mut attributes, "data-dirty", meta.dirty());

    attributes
}

fn use_field_meta_id_registration(meta: &FieldMeta, kind: RegisteredIdKind, id: Rc<str>) {
    let active = use_hook(|| Rc::new(RefCell::new(None::<ActiveFieldMetaIdRegistration>)));
    let should_replace = active
        .borrow()
        .as_ref()
        .is_none_or(|active| active.meta != *meta || active.kind != kind || active.id != id);

    if should_replace {
        let mut writable_meta = *meta;
        let registration = writable_meta.register_id(kind, id.clone());
        active.borrow_mut().replace(ActiveFieldMetaIdRegistration {
            meta: *meta,
            kind,
            id,
            _registration: registration,
        });
    }
}

struct ActiveFieldMetaIdRegistration {
    meta: FieldMeta,
    kind: RegisteredIdKind,
    id: Rc<str>,
    _registration: FieldMetaIdRegistration,
}

#[derive(Clone)]
struct BindingIdentity(Rc<dyn ComparableIdentity>);

impl BindingIdentity {
    fn new<I: PartialEq + 'static>(identity: I) -> Self {
        Self(Rc::new(identity))
    }
}

impl PartialEq for BindingIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.0.equals(other.0.as_ref())
    }
}

trait ComparableIdentity: Any {
    fn equals(&self, other: &dyn ComparableIdentity) -> bool;
}

impl<I: PartialEq + 'static> ComparableIdentity for I {
    fn equals(&self, other: &dyn ComparableIdentity) -> bool {
        let other = other as &dyn Any;
        other.downcast_ref::<I>().is_some_and(|other| self == other)
    }
}
