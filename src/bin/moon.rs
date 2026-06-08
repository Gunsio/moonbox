#![cfg_attr(
    not(test),
    deny(
        unsafe_code,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

use color_eyre::Result;

fn main() -> Result<()> {
    moonbox::run()
}
