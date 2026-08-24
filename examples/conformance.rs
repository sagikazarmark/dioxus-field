use std::{cell::RefCell, rc::Rc};

use dioxus::prelude::{dioxus_elements, rsx};
use dioxus_core::{Callback, Element, VirtualDom};
use dioxus_field::{
    Binding, ChangeOrigin,
    testing::{ChangeOriginProbe, CommitOrderProbe},
};
use dioxus_hooks::use_signal;
use dioxus_signals::ReadSignal;

#[derive(Clone, Default)]
struct Driver {
    binding: Rc<RefCell<Option<Binding<i32>>>>,
    on_submit: Rc<RefCell<Option<Callback<()>>>>,
}

#[derive(Clone)]
struct Harness {
    changes: ChangeOriginProbe<i32>,
    commits: CommitOrderProbe,
    driver: Driver,
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "VirtualDom entrypoints receive their root properties by value"
)]
fn app(harness: Harness) -> Element {
    let value = use_signal(|| 1);
    let binding = harness
        .changes
        .binding_with_commit(ReadSignal::from(value), harness.commits.on_commit());
    *harness.driver.binding.borrow_mut() = Some(binding.clone());
    *harness.driver.on_submit.borrow_mut() = Some(harness.commits.on_submit());

    rsx! { input { value: (binding.read)() } }
}

fn main() {
    let changes = ChangeOriginProbe::new();
    let commits = CommitOrderProbe::new();
    let driver = Driver::default();
    let mut dom = VirtualDom::new_with_props(
        app,
        Harness {
            changes: changes.clone(),
            commits: commits.clone(),
            driver: driver.clone(),
        },
    );
    dom.rebuild_in_place();

    dom.in_runtime(|| {
        let binding = driver
            .binding
            .borrow()
            .clone()
            .expect("app should store the binding");
        binding.write(2, ChangeOrigin::User);
        binding.commit();
        driver
            .on_submit
            .borrow()
            .expect("app should store its submit handler")
            .call(());
    });

    changes.assert_writes(&[(2, ChangeOrigin::User)]);
    commits.assert_commit_before_submit();
}
