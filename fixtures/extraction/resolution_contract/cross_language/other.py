"""Cross-language-name-collision (Python half): the ONLY definition of
``shared_widget`` lives here, in a different language than the Rust caller. The
same-language constraint must keep the Rust call unresolved."""


def shared_widget():
    pass
