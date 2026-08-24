# dioxus-field

[![ci](https://img.shields.io/github/actions/workflow/status/sagikazarmark/dioxus-field/ci.yaml?style=flat-square&label=ci)](https://github.com/sagikazarmark/dioxus-field/actions/workflows/ci.yaml)
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
- **Headless field parts**: render unstyled fields, labels, descriptions, and errors with coordinated
  ARIA attributes and focus requests.
- **Conformance testing**: verify widget registries through reusable probes without a browser renderer
  or form library.

## Quick Start

Wrap a normal Dioxus signal in a binding and provide it to a field-shaped widget:

```rust
use dioxus::prelude::*;
use dioxus_field::{Binding, Field, FieldContext};

fn app() -> Element {
    let mut name = use_signal(String::new);
    let binding: Binding<String> = name.into();

    rsx! {
        Field { binding: FieldContext::new(binding),
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
safe forwarding mechanism, and duplicate listeners on one element silently keep the first.

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

Keep these five named tests in every registry:

| Test | Kit API | Registry adapter responsibility |
| --- | --- | --- |
| `commit_is_synchronously_observable_before_submit_handling_runs` | `CommitOrderProbe` | Wire `on_commit()` to the widget commit path and `on_submit()` to the containing submit handler. |
| `writes_carry_their_change_origin` | `ChangeOriginProbe` | Give the produced binding to the widget and drive user and programmatic writes. |
| `binding_resolution_precedence_holds_for_values_and_meta_flags` | `assert_binding_resolution_precedence`, `assert_meta_resolution_precedence`, `assert_meta_flag_precedence` | Exercise explicit, Field Context, and internal sources; report flags from actual rendered state or attributes. |
| `focus_request_round_trips_to_the_widget_control` | `FocusRoundTripProbe` | Register `on_focus()` through the widget's normal focus registration and request focus through Field Context. |
| `error_and_description_ids_appear_on_mount_and_vanish_on_drop` | `assert_field_part_ids` | Mount and drop the registry's description and error parts around the same `FieldMeta`. |

The test adapter is intentionally registry-owned. It may dispatch DOM events or expose the same
handlers the rendered control uses, but it should not reproduce binding or metadata resolution in
test-only code. This keeps the assertions shared while allowing checkbox, select, slider, and other
widgets to retain their native interaction semantics.

The `testing` module documentation links to a runnable interaction-probe adapter, and
`tests/conformance.rs` is the reference implementation exercising all five tests against a minimal
conforming widget.

## Status

The crate is currently incubating and its API is not yet stable.

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
