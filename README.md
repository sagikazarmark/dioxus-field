# dioxus-field

[![ci](https://img.shields.io/github/actions/workflow/status/sagikazarmark/dioxus-field/dagger.yaml?style=flat-square&label=ci)](https://github.com/sagikazarmark/dioxus-field/actions/workflows/dagger.yaml)
[![openssf scorecard](https://api.securityscorecards.dev/projects/github.com/sagikazarmark/dioxus-field/badge?style=flat-square&label=openssf%20scorecard)](https://securityscorecards.dev/viewer/?uri=github.com/sagikazarmark/dioxus-field)
[![crates.io](https://img.shields.io/crates/v/dioxus-field?style=flat-square)](https://crates.io/crates/dioxus-field)
[![docs.rs](https://img.shields.io/docsrs/dioxus-field?style=flat-square)](https://docs.rs/dioxus-field)

**A form-library-agnostic field convention for Dioxus widget libraries.**

`dioxus-field` lets widget libraries accept reactive values, change callbacks, and interaction
commits without depending on a form library or prescribing rendered controls.

## Features

- **Two binding levels**: use a dependency-free `value` / `on_change` / `on_commit` prop trio or a
  reactive `Binding` that preserves the origin of each write.
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

Appending a name the vector does not already carry needs only
`sort_by(|left, right| left.name.cmp(right.name))` afterwards. To replace a value the metadata
supplied, set the matching override on `FieldControlOptions` instead of appending a second entry.
Controls merging through `merge_attributes` in `dioxus-primitives` need neither — it sorts and
dedupes every list it is given.

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

The test adapter is intentionally registry-owned. It may dispatch DOM events or expose the same
handlers the rendered control uses, but it should not reproduce binding or metadata resolution in
test-only code. This keeps the assertions shared while allowing checkbox, select, slider, and other
widgets to retain their native interaction semantics.

The `testing` module documentation links to a runnable interaction-probe adapter, and
`tests/conformance.rs` is the reference implementation exercising all six tests against a minimal
conforming widget.

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
