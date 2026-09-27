macro_rules! render {
    () => { 1 };
}

fn before_inner_scope() {
    render!();
}

fn scope_owner() {
    {
        macro_rules! render {
            () => { 2 };
        }
        render!();
    }
    render!();
}

fn qualified() {
    crate::render!();
}
