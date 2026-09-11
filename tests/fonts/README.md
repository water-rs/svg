# Deterministic test font

Roboto Regular, from the release the `water` CLI's font registry ships to
applications
(<https://github.com/googlefonts/roboto/releases/download/v2.138/roboto-android.zip>),
licensed under the Apache License 2.0 (see `LICENSE` beside the file).

The text fixtures are laid out against this file rather than whatever the host
OS happens to have installed, so `<text>` flattens into the same outlines on
every runner. A test that reached for the machine's font catalogue would be
asserting the state of the machine, not the behaviour of this crate.
