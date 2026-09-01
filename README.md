# dioxus-field

[![ci](https://img.shields.io/github/actions/workflow/status/sagikazarmark/dioxus-field/dagger.yaml?style=flat-square&label=ci)](https://github.com/sagikazarmark/dioxus-field/actions/workflows/dagger.yaml)
[![openssf scorecard](https://api.securityscorecards.dev/projects/github.com/sagikazarmark/dioxus-field/badge?style=flat-square&label=openssf%20scorecard)](https://securityscorecards.dev/viewer/?uri=github.com/sagikazarmark/dioxus-field)
[![crates.io](https://img.shields.io/crates/v/dioxus-field?style=flat-square)](https://crates.io/crates/dioxus-field)
[![docs.rs](https://img.shields.io/docsrs/dioxus-field?style=flat-square)](https://docs.rs/dioxus-field)

**A form-library-agnostic field convention for Dioxus widget libraries.**

`dioxus-field` lets widget libraries accept reactive values, change callbacks, interaction commits,
and logical focus exits without depending on a form library or prescribing rendered controls.

## Features

- **Two binding levels**: use a dependency-free `value` / `on_change` / `on_commit` prop trio or a
  reactive `Binding` that preserves each write's origin and independently reports Commit and Focus
  Exit.
- **Field metadata**: resolve signal-backed accessibility and interaction state from explicit props,
  Field Context, or standalone state.
- **Control attributes**: derive a control's `id`, `name`, state, and ARIA references in one call,
  spelled the way the rendered element accepts them.
- **Headless field parts**: render unstyled fields, labels, descriptions, and errors with coordinated
  ARIA attributes and focus requests.
- **Conformance testing**: verify widget registries through reusable probes without a browser renderer
  or form library.

## Quick Start

Provide a normal Dioxus signal through a `Field` to a field-shaped widget:

```rust
use dioxus::prelude::*;
use dioxus_field::Field;

fn app() -> Element {
    let mut name = use_signal(String::new);

    rsx! {
        Field { context: name,
            input {
                value: name,
                oninput: move |event| name.set(event.value()),
            }
        }
    }
}
```

The [complete field example](examples/field.rs) adds metadata, labels, descriptions, errors, and
derived accessibility attributes.

On Dioxus 0.7.10, forward listeners through an explicit `attributes: vec![...]` collection or an
explicit `Option<EventHandler<_>>` prop. Bare listener props passed through `extends` are not yet a
safe forwarding mechanism, and duplicate listeners on one element silently keep the first. Upstream
tracks this in [DioxusLabs/dioxus#4019](https://github.com/DioxusLabs/dioxus/issues/4019), resolved
on `main` by [DioxusLabs/dioxus#5554](https://github.com/DioxusLabs/dioxus/pull/5554) for the 0.8
release line.

## Control Attributes

A field-aware control resolves its metadata once, then asks for the attributes its element should
carry:

```rust
use dioxus_field::{FieldControlOptions, FieldSurface, use_field_meta};

let meta = use_field_meta(props.meta);
let attributes = meta.attributes_for(
    &FieldControlOptions::new()
        .required(props.required)
        .disabled(props.disabled)
        .name(props.name.clone())
        // A `button[role=switch]` takes native `disabled`, but `required` has no native
        // spelling there.
        .surface(FieldSurface::BUTTON_WIDGET),
);
```

Explicit props win over Field Context, which wins over standalone state. Resolution happens before
any attribute is built, so an overridden state is never emitted twice and the control never filters
the result.

`FieldSurface` carries one axis per attribute — `required`, `disabled`, `validity`, and `name` —
because their validity lattices disagree: native `disabled` is legal on a `button` where native
`required` is not, and `name` is not a valid attribute on the `div` a radio group roots on. Presets
cover the common elements: `NATIVE` for `input` / `textarea` / `select`, `BUTTON_WIDGET` for
`button[role=checkbox|switch]`, and `ARIA_WIDGET` for `div[role=radiogroup]`.

The returned vector is sorted by attribute name and holds at most one entry per name. The sort is
what `dioxus-core` requires of any spread — its attribute diff is a sorted merge-join, so an
unsorted spread makes a later render drop attributes that did not change. The single entry per name
guards the neighbouring failure, where a duplicate that drops to one deletes the attribute outright.

To combine that with a widget's own attributes, hand both to `merge_attributes`:

```rust
use dioxus_field::merge_attributes;

let merged = merge_attributes(vec![
    meta.attributes_for(&options),
    base_attributes,
    explicit_attributes,
    props.attributes,
]);
```

Groups resolve last-wins, ordered weakest to strongest, and the result keeps the sort and the
one-entry-per-name guarantee. `class` is the exception: values are concatenated weakest-first, so a
widget's own classes survive a caller's.

Pass ordered groups rather than one pre-concatenated list: merging the metadata into the explicit
props before the call moves the widget's base attributes past both, so base silently outranks an
explicit `name` or `required` it was meant to lose to. To replace a value the metadata supplied, set
the matching override on `FieldControlOptions` instead of adding a second entry. Widgets already
merging through `merge_attributes` in `dioxus-primitives` do not need this one — that helper also
sorts, deduplicates, and concatenates `class`.

## Wrong-Type Binding Diagnostics

A present Field Context binding of the wrong value type never falls back to standalone state: the
resolving control panics. Immediately before that panic, this crate emits an ERROR-level `tracing`
event so the type names reach a structured channel. The stable observability surface is the event
target `dioxus_field`, the fields `actual`, `requested`, `field_id`, and `field_name`, and the
message substrings `Field Context binding type mismatch` and
`Field Context contains no value binding`. `dioxus::launch` installs a tracing subscriber by
default on both native and web, so a stock app sees the event without wiring anything.

The event matters because what the panic itself shows depends on the platform and build profile:

- **Native**: Dioxus catches render panics, so the control's subtree is simply absent while
  siblings render. std's panic hook prints the message once to stderr, easily lost in `dx serve`
  scroll, and the framework's own log line names only the component, not the mismatched types. An
  `ErrorBoundary` shows a fallback but cannot recover the message.
- **wasm dev builds**: the panic aborts the app mid-render; dioxus-web's devtools panic hook logs
  the message to the console with a toast.
- **wasm release builds**: a dead app with no message at all — the tracing event is the only
  diagnostic. Apps that compile tracing with `release_max_level_off`-style features compile the
  event out, which is their explicit choice.

Wrapping the app in an `ErrorBoundary` at least makes the failure visible on native. The panic
message appends the field id and name (when available) after the unchanged leading sentence, so
existing `should_panic` matchers on the leading sentence keep working.

## Conformance Testing

Widget registries can use the public `dioxus_field::testing` module from ordinary integration tests;
no browser renderer or form library is required. The convention has two conformance levels, and a
registry states which one each widget meets:

- **Trio-conformant**: the widget honors the `value` / `on_change` / `on_commit` prop trio plus
  attribute spread, with no dependency on this crate. Applicable tests are commit ordering and change
  origin; trio-only widgets imply the user origin.
- **Field-aware**: the widget additionally resolves the Field Context. Applicable tests are the three
  resolution-precedence assertions, the focus round-trip, and part-id registration. Create the probe
  outside the `VirtualDom`, obtain its Dioxus callbacks while rendering the test component, drive the
  registry component through its normal interaction path, then call the assertion after rendering.

Keep these six named tests in every registry:

| Test | Kit API | Registry adapter responsibility |
| --- | --- | --- |
| `commit_is_synchronously_observable_before_submit_handling_runs` | `CommitOrderProbe` | Wire `on_commit()` to the widget commit path and `on_submit()` to the containing submit handler. |
| `writes_carry_their_change_origin` | `ChangeOriginProbe` | Give the produced binding to the widget and drive user and programmatic writes. |
| `binding_resolution_precedence_holds_for_values_and_meta_flags` | `assert_binding_resolution_precedence`, `assert_meta_resolution_precedence`, `assert_meta_flag_precedence` | Exercise explicit, Field Context, and internal sources; report flags from actual rendered state or attributes. |
| `focus_request_round_trips_to_the_widget_control` | `FocusRoundTripProbe` | Register `on_focus()` through the widget's normal focus registration and request focus through Field Context. |
| `a_focus_request_does_not_move_focus_while_the_control_is_disabled` | `FocusRoundTripProbe::assert_focus_not_moved` | Request focus while the widget is disabled. A disabled control focuses nothing rather than handing focus to a proxy element. |
| `error_and_description_ids_appear_on_mount_and_vanish_on_drop` | `assert_field_part_ids` | Mount and drop the registry's description and error parts around the same `FieldMeta`. |

Bindings that opt into Focus Exit can add these tests without changing either conformance level:

| Test | Kit API | Registry adapter responsibility |
| --- | --- | --- |
| `a_reported_focus_exit_is_observable_exactly_once` | `FocusExitProbe` | Wire `on_focus_exit()` through `Binding::with_focus_exit`, leave the widget's complete logical focus scope once, and assert one report. |
| `internal_focus_movement_does_not_report_focus_exit` | `FocusExitProbe::assert_no_focus_exit` | Move focus between owned controls or popup/portal content without leaving the logical scope. |
| `commit_without_focus_exit_remains_valid` | `FocusExitOrderProbe` | Drive an interaction that commits while focus remains in the widget. |
| `focus_exit_is_observed_after_synchronous_write_and_commit` | `FocusExitOrderProbe` | Drive the widget's normal write, Commit, then Focus Exit path and assert the observed order. |

The registry defines the complete logical focus scope and owns its detection and deduplication.
Controls that implement only the Binding Prop Trio remain exempt from Focus Exit conformance.

The test adapter is intentionally registry-owned. It may dispatch DOM events or expose the same
handlers the rendered control uses, but it should not reproduce binding or metadata resolution in
test-only code. This keeps the assertions shared while allowing checkbox, select, slider, and other
widgets to retain their native interaction semantics.

The `testing` module documentation links to a runnable interaction-probe adapter, and
`tests/conformance.rs` is the reference implementation exercising all six required tests plus the
optional Focus Exit tests against a minimal conforming widget.

## Development

Run the local Rust checks with:

```shell
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --doc
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps
```

Run the same repository gate used by CI with:

```shell
dagger check
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
