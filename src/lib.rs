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

use std::{
    any::Any,
    cell::{Cell, RefCell},
    fmt,
    rc::Rc,
};

use dioxus::prelude::{Props, dioxus_elements, rsx};
use dioxus_core::{
    Attribute, Callback, Element, current_scope_id, has_context, provide_context,
    try_consume_context, use_hook,
};
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

/// Explicit state that wins over the resolved metadata's own state.
///
/// This is the whole override set for a field part, and the state subset of the control path
/// carried by [`FieldControlOptions`]. A `None` field defers to the metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldStateOverrides {
    /// Overrides the metadata's invalid state when present.
    pub invalid: Option<bool>,
    /// Overrides the metadata's disabled state when present.
    pub disabled: Option<bool>,
    /// Overrides the metadata's required state when present.
    pub required: Option<bool>,
}

/// How one rendered element spells a field attribute that has both a native and an ARIA form.
///
/// The question this answers is *attribute applicability* — does this element accept this
/// attribute — not which accessibility spelling reads better. Only the widget knows the element it
/// renders and the role that element carries, so the widget supplies it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttributeSurface {
    /// Emit the native HTML attribute, such as `required` or `disabled`.
    #[default]
    Native,
    /// Emit the ARIA attribute, such as `aria-required` or `aria-disabled`.
    Aria,
    /// Emit neither spelling.
    ///
    /// Use this where the attribute is invalid on the rendered element and no ARIA spelling
    /// applies to its role either.
    Omit,
}

/// How one rendered element exposes validity.
///
/// Validity has no native spelling, so this axis has no `Native` variant. It gates `aria-invalid`
/// and `aria-errormessage` together, since a validity reference without a validity state is not
/// meaningful.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValiditySurface {
    /// Emit `aria-invalid`, and `aria-errormessage` while invalid.
    #[default]
    Aria,
    /// Emit neither, for roles where `aria-invalid` is unsupported or deprecated.
    Omit,
}

/// Whether one rendered element accepts the `name` attribute.
///
/// `name` has no ARIA spelling, so this axis has no `Aria` variant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum NameSurface {
    /// Emit `name`.
    #[default]
    Native,
    /// Emit nothing.
    ///
    /// Controls rooted on a `div` need this: `name` is not a valid attribute there, and such
    /// controls do not participate in native form submission.
    Omit,
}

/// How one rendered element spells its field state, one axis per attribute.
///
/// The axes are independent because their validity lattices disagree pairwise: native `disabled`
/// is legal on a `button` where native `required` is not, and `aria-invalid` is unsupported on
/// some roles where `aria-disabled` is fine. A single index over all of them cannot describe any
/// element correctly.
///
/// The `data-*` state attributes are outside this type. They are valid on every element, so
/// [`FieldMeta::attributes_for`] always emits them regardless of the surface — an `Omit` axis
/// suppresses only the attribute the element cannot carry, never the styling hook.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldSurface {
    /// How the element spells its required state.
    pub required: AttributeSurface,
    /// How the element spells its disabled state.
    pub disabled: AttributeSurface,
    /// How the element spells its validity.
    pub validity: ValiditySurface,
    /// Whether the element carries a `name`.
    pub name: NameSurface,
}

impl FieldSurface {
    /// `input`, `textarea`, and `select` — every axis that *has* a native spelling uses it, and
    /// this is the default. Validity stays ARIA, since no element spells validity natively.
    pub const NATIVE: Self = Self {
        required: AttributeSurface::Native,
        disabled: AttributeSurface::Native,
        validity: ValiditySurface::Aria,
        name: NameSurface::Native,
    };

    /// `button[role=checkbox|switch]` — native `disabled` and `name` are legal on a `button`,
    /// native `required` is not.
    pub const BUTTON_WIDGET: Self = Self {
        required: AttributeSurface::Aria,
        disabled: AttributeSurface::Native,
        validity: ValiditySurface::Aria,
        name: NameSurface::Native,
    };

    /// `div[role=radiogroup]` — no native attribute applies, and a `div` carries no `name`.
    ///
    /// A control rooted on `role=group` should start here and set `validity` to
    /// [`ValiditySurface::Omit`], since `aria-invalid` is deprecated on that role.
    pub const ARIA_WIDGET: Self = Self {
        required: AttributeSurface::Aria,
        disabled: AttributeSurface::Aria,
        validity: ValiditySurface::Aria,
        name: NameSurface::Omit,
    };
}

/// Everything a field-aware control tells [`FieldMeta::attributes_for`] about itself.
///
/// Overrides are resolved before any attribute is built, so an overridden state is never emitted
/// twice and never has to be filtered back out of the result.
///
/// ```rust
/// # use std::rc::Rc;
/// # use dioxus_field::{FieldControlOptions, FieldSurface};
/// let options = FieldControlOptions::new()
///     .required(Some(true))
///     .name(Some(Rc::from("terms")))
///     .surface(FieldSurface::BUTTON_WIDGET);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldControlOptions {
    state: FieldStateOverrides,
    id: Option<Rc<str>>,
    name: Option<Rc<str>>,
    surface: FieldSurface,
}

impl FieldControlOptions {
    /// Creates options that override nothing and render onto [`FieldSurface::NATIVE`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the whole state override set.
    #[must_use]
    pub fn state(mut self, state: FieldStateOverrides) -> Self {
        self.state = state;
        self
    }

    /// Overrides the metadata's invalid state.
    #[must_use]
    pub fn invalid(mut self, invalid: Option<bool>) -> Self {
        self.state.invalid = invalid;
        self
    }

    /// Overrides the metadata's disabled state.
    #[must_use]
    pub fn disabled(mut self, disabled: Option<bool>) -> Self {
        self.state.disabled = disabled;
        self
    }

    /// Overrides the metadata's required state.
    #[must_use]
    pub fn required(mut self, required: Option<bool>) -> Self {
        self.state.required = required;
        self
    }

    /// Overrides the metadata's control id.
    ///
    /// This replaces the emitted value; it does not suppress the attribute. `id` is a global
    /// attribute, so there is no element that must not carry one.
    #[must_use]
    pub fn id(mut self, id: Option<Rc<str>>) -> Self {
        self.id = id;
        self
    }

    /// Overrides the metadata's control name.
    ///
    /// This replaces the emitted value. To suppress the attribute on an element that cannot carry
    /// it, set [`FieldSurface::name`] to [`NameSurface::Omit`] instead.
    #[must_use]
    pub fn name(mut self, name: Option<Rc<str>>) -> Self {
        self.name = name;
        self
    }

    /// Declares how the rendered element spells each field attribute.
    #[must_use]
    pub fn surface(mut self, surface: FieldSurface) -> Self {
        self.surface = surface;
        self
    }
}

/// Signal-backed presentation metadata for one field-shaped value.
///
/// The flag meanings are producer-defined. This type does not track an initial value or classify
/// validity. Error strings are already formatted for display before they cross this boundary.
#[derive(Clone, Copy, PartialEq)]
pub struct FieldMeta {
    id: Signal<Option<Rc<str>>>,
    fallback_id: Signal<Rc<str>>,
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
        let id = self
            .id
            .peek()
            .clone()
            .unwrap_or_else(|| self.fallback_id.peek().clone());

        f.debug_struct("FieldMeta")
            .field("id", &id)
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
    ///
    /// Metadata always carries an id. When its producer supplies none,
    /// [`use_field_meta_state`] generates one that is stable for the owning scope's lifetime, so
    /// [`Label`]'s `for`, `aria-labelledby`, `aria-describedby`, and `aria-errormessage` all
    /// resolve instead of silently leaving the control unnamed.
    pub fn id(&self) -> Rc<str> {
        (self.id)().unwrap_or_else(|| (self.fallback_id)())
    }

    /// Replaces the rendered control id, or restores the generated fallback with `None`.
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

    /// Registers a label element id until the returned registration is dropped.
    ///
    /// Registered label ids reach the control through `aria-labelledby`, which is the only naming
    /// path available to a control rooted on an element `<label for>` cannot address.
    #[must_use]
    pub fn register_label_id(&mut self, id: Rc<str>) -> FieldMetaIdRegistration {
        self.register_id(RegisteredIdKind::Label, id)
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

    /// Returns attributes for a rendered control using the metadata's own state, on a
    /// [`FieldSurface::NATIVE`] element.
    pub fn attributes(&self) -> Vec<Attribute> {
        self.attributes_for(&FieldControlOptions::default())
    }

    /// Returns attributes for a rendered control, resolving explicit overrides and the element's
    /// attribute surface.
    ///
    /// Overrides are resolved first and only the resolved state is emitted, so the caller never
    /// filters the result and no attribute appears twice.
    ///
    /// # Guarantees
    ///
    /// The returned vector is **sorted by attribute name** and carries at most one entry per
    /// attribute name and namespace.
    ///
    /// The sort is what `dioxus-core` requires of any spread list: its attribute diff is a sorted
    /// merge-join, so an unsorted list makes a later render drop attributes that did not change.
    /// The single entry per name guards the neighbouring failure, where a spread carrying one name
    /// twice and dropping to once emits a removal, deleting an attribute the new render still has.
    ///
    /// To combine this with a widget's own attributes, pass both to [`merge_attributes`], which
    /// preserves the guarantee and resolves each name last-wins. To *replace* a value the metadata
    /// supplied, set the matching override on [`FieldControlOptions`] rather than adding a second
    /// entry — that is what the overrides are for.
    ///
    /// # Emitted attributes
    ///
    /// - `id`, always, from the override or the metadata.
    /// - `name`, when [`FieldSurface::name`] is [`NameSurface::Native`] and a name resolves.
    /// - `required` or `aria-required="true"`, when required, per [`FieldSurface::required`].
    /// - `disabled` or `aria-disabled="true"`, when disabled, per [`FieldSurface::disabled`].
    /// - `aria-invalid`, and `aria-errormessage` while invalid, per [`FieldSurface::validity`].
    ///   `aria-errormessage` takes a single IDREF in ARIA 1.2, so it references only the first
    ///   mounted error part; every error id also reaches `aria-describedby` while invalid.
    /// - `aria-labelledby` and `aria-describedby`, from the currently mounted parts. Both are
    ///   legal on every role in play, so neither has a surface axis.
    /// - `data-required`, `data-disabled`, `data-invalid`, `data-touched`, and `data-dirty`, from
    ///   the resolved state, absent when false and independent of the surface.
    pub fn attributes_for(&self, options: &FieldControlOptions) -> Vec<Attribute> {
        let required = options.state.required.unwrap_or_else(|| self.required());
        let disabled = options.state.disabled.unwrap_or_else(|| self.disabled());
        let invalid = options.state.invalid.unwrap_or_else(|| self.invalid());
        let id = options.id.clone().unwrap_or_else(|| self.id());
        let name = options.name.clone().or_else(|| self.name());
        let registered_ids = (self.registered_ids)();
        let error_ids = registered_ids.ids(RegisteredIdKind::Error);
        let mut described_by = registered_ids.ids(RegisteredIdKind::Description);
        if invalid {
            described_by.extend(error_ids.iter().cloned());
        }
        let mut attributes = Vec::new();

        attributes.push(Attribute::new("id", id.to_string(), None, false));

        if options.surface.name == NameSurface::Native {
            push_optional_text(&mut attributes, "name", name);
        }

        push_surface_state(
            &mut attributes,
            options.surface.required,
            ("required", "aria-required"),
            required,
        );
        push_surface_state(
            &mut attributes,
            options.surface.disabled,
            ("disabled", "aria-disabled"),
            disabled,
        );

        if options.surface.validity == ValiditySurface::Aria {
            attributes.push(Attribute::new(
                "aria-invalid",
                invalid.to_string(),
                None,
                false,
            ));
            if invalid {
                push_optional_text(
                    &mut attributes,
                    "aria-errormessage",
                    error_ids.first().cloned(),
                );
            }
        }

        push_optional_text(
            &mut attributes,
            "aria-labelledby",
            join_ids(&registered_ids.ids(RegisteredIdKind::Label)),
        );
        push_optional_text(&mut attributes, "aria-describedby", join_ids(&described_by));

        push_state(&mut attributes, "data-required", required);
        push_state(&mut attributes, "data-disabled", disabled);
        push_state(&mut attributes, "data-invalid", invalid);
        push_state(&mut attributes, "data-touched", self.touched());
        push_state(&mut attributes, "data-dirty", self.dirty());

        normalize_attributes(&mut attributes);

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
///
/// When `initial.id` is `None`, the metadata falls back to an id generated for this hook, stable
/// for the owning scope's lifetime. Setting the id back to `None` later restores that fallback, so
/// a control resolved through this metadata always has an id to be labelled and described by.
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
        fallback_id: use_signal(|| generated_id("field")),
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
    Label,
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

    /// Returns the ids of one kind in registration order, which is the order ARIA id references
    /// are rendered in.
    fn ids(&self, kind: RegisteredIdKind) -> Vec<Rc<str>> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == kind)
            .map(|entry| Rc::clone(&entry.id))
            .collect()
    }
}

fn join_ids(ids: &[Rc<str>]) -> Option<Rc<str>> {
    (!ids.is_empty()).then(|| {
        Rc::from(
            ids.iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>()
                .join(" ")
                .as_str(),
        )
    })
}

/// A per-scope counter that keeps generated ids unique within one component.
#[derive(Clone)]
struct GeneratedIdCounter(Rc<Cell<u64>>);

/// Generates an id unique to the calling scope and to this call's position within it.
///
/// Call this only from a hook initializer so the value is computed once and stays stable for the
/// scope's lifetime. The counter lives in the scope's own context, which keeps generated ids
/// deterministic for a given [`dioxus_core::VirtualDom`] rather than dependent on global state.
fn generated_id(prefix: &str) -> Rc<str> {
    let counter = has_context::<GeneratedIdCounter>()
        .unwrap_or_else(|| provide_context(GeneratedIdCounter(Rc::new(Cell::new(0)))));
    let index = counter.0.get();
    counter.0.set(index + 1);

    Rc::from(format!("dxf-{prefix}-{}-{index}", current_scope_id().0).as_str())
}

/// Resolves a field part's own element id, generating a stable one when the caller supplies none.
fn use_part_id(explicit: Option<Rc<str>>, prefix: &'static str) -> Rc<str> {
    let generated = use_hook(|| generated_id(prefix));

    explicit.unwrap_or(generated)
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

/// Pushes one state in the spelling its surface calls for, and nothing when `value` is false.
///
/// The native spelling is a boolean attribute; the ARIA one is `="true"`. Both are absent when
/// false, so a selector never has to distinguish `false` from unset.
fn push_surface_state(
    attributes: &mut Vec<Attribute>,
    surface: AttributeSurface,
    (native, aria): (&'static str, &'static str),
    value: bool,
) {
    match surface {
        AttributeSurface::Native => push_bool(attributes, native, value),
        AttributeSurface::Aria => push_state(attributes, aria, value),
        AttributeSurface::Omit => {}
    }
}

/// Pushes `name="true"` when `value`, and nothing otherwise.
///
/// Both the `data-*` state attributes and the ARIA states this crate emits use the same
/// absent-when-false convention, so a selector never has to distinguish `false` from unset.
fn push_state(attributes: &mut Vec<Attribute>, name: &'static str, value: bool) {
    if value {
        attributes.push(Attribute::new(name, "true", None, false));
    }
}

/// Merges ordered attribute groups into one list a widget can spread.
///
/// Groups are resolved **last-wins**: where two groups set the same attribute name and namespace,
/// the later group's value survives. Order the groups from weakest to strongest — for a
/// field-aware control that is typically the metadata attributes, then the widget's own base
/// attributes, then its explicit props, then the caller's forwarded attributes.
///
/// Passing ordered groups rather than one pre-concatenated list is the point. Concatenating the
/// metadata and explicit groups before the call moves the widget's base attributes past both, so
/// base silently outranks an explicit `name` or `required` it was meant to lose to.
///
/// The result carries the same guarantee as [`FieldMeta::attributes_for`]: sorted by attribute
/// name, at most one entry per name and namespace. `dioxus-core` requires the sort of any spread,
/// and the deduplication keeps a name that appears twice and later drops to once from deleting the
/// attribute outright.
///
/// Widgets already merging through `merge_attributes` in `dioxus-primitives` do not need this; it
/// resolves groups the same way.
///
/// ```rust
/// # use dioxus_core::Attribute;
/// # use dioxus_field::merge_attributes;
/// let merged = merge_attributes(vec![
///     vec![Attribute::new("name", "from-meta", None, false)],
///     vec![Attribute::new("name", "from-explicit", None, false)],
/// ]);
///
/// assert_eq!(merged.len(), 1);
/// ```
pub fn merge_attributes(groups: Vec<Vec<Attribute>>) -> Vec<Attribute> {
    let mut attributes = groups.into_iter().flatten().collect::<Vec<_>>();
    normalize_attributes(&mut attributes);

    attributes
}

/// Sorts by attribute name and keeps the last entry for each name and namespace.
///
/// `dioxus-core` diffs a spread attribute list with a sorted merge-join keyed on the attribute
/// name, so an unsorted or duplicated list makes the next render emit removals for attributes that
/// are still present. Every list this crate hands to `rsx!` passes through here, including after
/// caller attributes are appended — appending last is what makes a caller's attribute win its
/// name.
fn normalize_attributes(attributes: &mut Vec<Attribute>) {
    attributes.sort_by(|left, right| {
        left.name
            .cmp(right.name)
            .then_with(|| left.namespace.cmp(&right.namespace))
    });
    attributes.dedup_by(|later, earlier| {
        let duplicate = later.name == earlier.name && later.namespace == earlier.namespace;

        if duplicate {
            std::mem::swap(later, earlier);
        }

        duplicate
    });
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
/// absent context from a present context containing the wrong value type, and lets [`Field`]
/// accept any value type without becoming generic itself.
///
/// # Equality
///
/// Two contexts are equal when their bindings are equal under [`Binding`]'s identity equality and
/// their metadata is equal, regardless of when either context was constructed. The focus request
/// slot is intentionally excluded: [`Field`] pins the slot of the first context it receives for
/// its lifetime, so the slot carried by a context built on a later render is never observed by
/// descendants, and comparing it would only defeat memoization.
#[derive(Clone)]
pub struct FieldContext {
    binding: Option<ErasedBinding>,
    meta: Option<FieldMeta>,
    meta_values: Option<FieldMetaValues>,
    focus_request: FocusRequest,
}

impl FieldContext {
    /// Creates context for a binding.
    pub fn new<T: 'static>(binding: Binding<T>) -> Self {
        Self {
            binding: Some(ErasedBinding::new(binding)),
            meta: None,
            meta_values: None,
            focus_request: FocusRequest::default(),
        }
    }

    /// Creates context with no value binding or metadata.
    pub fn empty() -> Self {
        Self {
            binding: None,
            meta: None,
            meta_values: None,
            focus_request: FocusRequest::default(),
        }
    }

    /// Replaces the context's value binding.
    #[must_use]
    pub fn with_binding<T: 'static>(mut self, binding: Binding<T>) -> Self {
        self.binding = Some(ErasedBinding::new(binding));
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
    ///
    /// [`Field`] pins the slot of the first context it receives, so producers that request focus
    /// through a context must keep that context stable across renders.
    pub fn focus_request(&self) -> FocusRequest {
        self.focus_request.clone()
    }

    /// Resolves the context binding for `T`.
    ///
    /// # Panics
    ///
    /// Panics when the field context contains no binding or a binding for a different value type.
    pub fn resolve<T: 'static>(&self) -> Binding<T> {
        let erased = self
            .binding
            .as_ref()
            .unwrap_or_else(|| panic!("Field Context contains no value binding"));

        erased
            .binding
            .downcast_ref::<Binding<T>>()
            .unwrap_or_else(|| {
                panic!(
                    "Field Context contains a binding for {}, but a binding for {} was requested",
                    erased.value_type_name,
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
            .field(
                "value_type_name",
                &self.binding.as_ref().map(|binding| binding.value_type_name),
            )
            .field("meta", &self.meta)
            .field("meta_values", &self.meta_values)
            .field("focus_request", &self.focus_request)
            .finish_non_exhaustive()
    }
}

impl PartialEq for FieldContext {
    fn eq(&self, other: &Self) -> bool {
        self.binding == other.binding
            && self.meta == other.meta
            && self.meta_values == other.meta_values
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

/// A value binding erased to `dyn Any` together with the comparator for its concrete type.
///
/// The comparator is captured at erasure time so [`FieldContext`] equality can delegate to
/// [`Binding`]'s identity equality instead of comparing wrapper allocations.
#[derive(Clone)]
struct ErasedBinding {
    binding: Rc<dyn Any>,
    value_type_name: &'static str,
    eq: fn(&dyn Any, &dyn Any) -> bool,
}

impl ErasedBinding {
    fn new<T: 'static>(binding: Binding<T>) -> Self {
        Self {
            binding: Rc::new(binding),
            value_type_name: std::any::type_name::<T>(),
            eq: |left, right| match (
                left.downcast_ref::<Binding<T>>(),
                right.downcast_ref::<Binding<T>>(),
            ) {
                (Some(left), Some(right)) => left == right,
                _ => false,
            },
        }
    }
}

impl PartialEq for ErasedBinding {
    fn eq(&self, other: &Self) -> bool {
        (self.eq)(&*self.binding, &*other.binding)
    }
}

/// Provides a binding as the current scope's [`FieldContext`].
///
/// The provided context keeps the focus request slot of the context this scope provided on an
/// earlier render, so widgets that memoized on that render stay registered with the slot producers
/// observe.
pub fn provide_field_context<T: 'static>(binding: Binding<T>) -> FieldContext {
    let mut context = FieldContext::new(binding);

    if let Some(existing) = has_context::<FieldContext>() {
        context = context.with_focus_request(existing.focus_request());
    }

    provide_context(context)
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
    use_resolved_field_meta(explicit.as_ref()).0
}

/// Where a part's resolved metadata came from.
///
/// A part that renders a reference *to a control* needs this: an id resolved from metadata nobody
/// else holds addresses no rendered element, so the reference would dangle.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldMetaSource {
    /// An explicit prop or the Field Context. A control may hold the same metadata, and if the
    /// caller passed it deliberately, one is meant to.
    Shared,
    /// This part's own standalone state, which by construction no control is reading.
    Standalone,
}

/// Resolves metadata as [`use_field_meta`] does, and reports which source won.
fn use_resolved_field_meta(explicit: Option<&FieldMeta>) -> (FieldMeta, FieldMetaSource) {
    let internal = use_field_meta_state(FieldMetaValues::default());

    explicit
        .copied()
        .or_else(|| try_consume_context::<FieldContext>().and_then(|context| context.meta()))
        .map_or((internal, FieldMetaSource::Standalone), |meta| {
            (meta, FieldMetaSource::Shared)
        })
}

/// Resolves the current [`FocusRequest`], or creates a standalone slot when no context exists.
pub fn use_focus_request() -> FocusRequest {
    let internal = use_hook(FocusRequest::default);

    try_consume_context::<FieldContext>().map_or(internal, |context| context.focus_request())
}

/// Registers a widget focus callback with the resolved [`FocusRequest`] for this component's
/// lifetime.
///
/// # The callback must be render-stable
///
/// Pass a callback whose identity survives a re-render — [`dioxus_hooks::use_callback`] produces
/// one. [`Callback::new`] does not: it allocates a fresh generational box per call and compares by
/// pointer identity, so calling it in a component body re-registers on **every** render. One slot
/// is shared by the whole field, so a widget that re-registers steals the slot from a sibling
/// widget that legitimately owns it, and focus ownership ends up decided by render recency rather
/// than by structure. Each re-registration also leaks a generational box until the component
/// unmounts.
///
/// # Which element to register
///
/// Register the element that actually receives focus, and let the callback do nothing when that
/// element cannot take focus — a disabled control should not move focus at all. Never focus a
/// proxy element and never blur: both hand the user's focus to something they did not ask for,
/// and `HTMLElement.focus()` reports success either way, so nothing downstream can detect it.
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
    /// Attributes forwarded to the rendered `div`.    ///
    /// Sorted by attribute name and deduplicated with the part's own, which `dioxus-core` requires
    /// of any spread list. A forwarded attribute wins its name.
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
    /// Field content.
    pub children: Element,
}

/// Provides one [`FieldContext`] and renders an unstyled `div` around its children.
///
/// On Dioxus 0.7.10, pass listeners through an explicit `attributes: vec![...]` prop so listener
/// ordering remains visible at the call site.
///
/// # Memoization
///
/// Children authored inline in `rsx!` and inline listener attributes compare unequal on every
/// parent render, so a `Field` receiving either re-renders with its parent regardless of
/// [`FieldContext`] equality. Forwarding a received element through the `children` prop keeps it
/// comparable; context equality then decides whether `Field` re-renders.
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
    let mut attributes = props.attributes;
    normalize_attributes(&mut attributes);

    rsx! {
        div { ..attributes, {props.children} }
    }
}

/// Props for the headless [`Label`] part.
#[derive(Clone, Debug, Props, PartialEq)]
pub struct LabelProps {
    /// The rendered `label`'s own id, registered with the resolved metadata for this part's
    /// lifetime. Defaults to a generated id.
    ///
    /// The control reaches this id through `aria-labelledby`, which is the only naming path
    /// available to a control rooted on an element `<label for>` cannot address — `for` requires a
    /// labelable element, and widgets rooted on a `div` are not one.
    #[props(default)]
    pub id: Option<Rc<str>>,
    /// Explicit metadata, which wins over Field Context metadata.
    #[props(default)]
    pub meta: Option<FieldMeta>,
    /// Explicit invalid state, which wins over the metadata state.
    #[props(default)]
    pub invalid: Option<bool>,
    /// Explicit disabled state, which wins over the metadata state.
    #[props(default)]
    pub disabled: Option<bool>,
    /// Explicit required state, which wins over the metadata state.
    #[props(default)]
    pub required: Option<bool>,
    /// Attributes forwarded to the rendered `label`.    ///
    /// Sorted by attribute name and deduplicated with the part's own, which `dioxus-core` requires
    /// of any spread list. A forwarded attribute wins its name.
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
    /// Label content.
    pub children: Element,
}

/// Renders an unstyled `label` associated with the resolved metadata's control id.
///
/// This part can resolve metadata from Field Context, accept it explicitly, or run standalone.
/// Running standalone means no control shares the metadata, so no `for` is emitted — the label
/// still carries its own id, which is the reference a control would reach through
/// `aria-labelledby`.
#[allow(non_snake_case)]
#[allow(
    clippy::missing_errors_doc,
    reason = "Dioxus Element uses Result as its renderer protocol"
)]
pub fn Label(props: LabelProps) -> Element {
    let (meta, source) = use_resolved_field_meta(props.meta.as_ref());
    let id = use_part_id(props.id, "label");
    use_field_meta_id_registration(&meta, RegisteredIdKind::Label, Rc::clone(&id));
    // Metadata the label resolved for itself alone addresses no rendered control, so pointing
    // `for` at its generated id would dangle. Emitting nothing is what 0.1.0 did, and is honest.
    let control_id = (source == FieldMetaSource::Shared).then(|| meta.id().to_string());
    let attributes = part_attributes(
        &meta,
        FieldStateOverrides {
            invalid: props.invalid,
            disabled: props.disabled,
            required: props.required,
        },
        props.attributes,
    );

    rsx! {
        label { id: id.to_string(), r#for: control_id, ..attributes, {props.children} }
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
    /// Explicit required state, which wins over the metadata state.
    #[props(default)]
    pub required: Option<bool>,
    /// Attributes forwarded to the rendered description `div`.    ///
    /// Sorted by attribute name and deduplicated with the part's own, which `dioxus-core` requires
    /// of any spread list. A forwarded attribute wins its name.
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
    let attributes = part_attributes(
        &meta,
        FieldStateOverrides {
            invalid: props.invalid,
            disabled: props.disabled,
            required: props.required,
        },
        props.attributes,
    );

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
    /// Explicit required state used by data-state attributes.
    #[props(default)]
    pub required: Option<bool>,
    /// Attributes forwarded to the rendered error `div`.    ///
    /// Sorted by attribute name and deduplicated with the part's own, which `dioxus-core` requires
    /// of any spread list. A forwarded attribute wins its name.
    #[props(extends = GlobalAttributes)]
    pub attributes: Vec<Attribute>,
}

/// Renders pre-formatted field errors in an unstyled polite live region, one element per error.
///
/// The live region stays mounted while the field is valid, holding no children. A live region that
/// enters the accessibility tree in the same update as its content is not announced reliably, so
/// the region has to exist before the first error arrives.
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
    let id = props.id.to_string();
    let errors = if invalid { meta.errors() } else { Vec::new() };
    let attributes = part_attributes(
        &meta,
        FieldStateOverrides {
            invalid: props.invalid,
            disabled: props.disabled,
            required: props.required,
        },
        props.attributes,
    );

    rsx! {
        div {
            id: id,
            aria_live: "polite",
            ..attributes,
            for error in errors {
                div { "{error}" }
            }
        }
    }
}

/// Builds a field part's `data-*` state attributes and appends the caller's forwarded ones.
///
/// Forwarded attributes go last, so a caller's attribute wins its name once
/// [`normalize_attributes`] resolves the result.
fn part_attributes(
    meta: &FieldMeta,
    overrides: FieldStateOverrides,
    forwarded: Vec<Attribute>,
) -> Vec<Attribute> {
    let required = overrides.required.unwrap_or_else(|| meta.required());
    let disabled = overrides.disabled.unwrap_or_else(|| meta.disabled());
    let invalid = overrides.invalid.unwrap_or_else(|| meta.invalid());
    let mut attributes = Vec::new();

    push_state(&mut attributes, "data-required", required);
    push_state(&mut attributes, "data-disabled", disabled);
    push_state(&mut attributes, "data-invalid", invalid);
    push_state(&mut attributes, "data-touched", meta.touched());
    push_state(&mut attributes, "data-dirty", meta.dirty());

    attributes.extend(forwarded);
    normalize_attributes(&mut attributes);

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
