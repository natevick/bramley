use ui_baseline::variant::{self, Parts};

fn main() -> eframe::Result {
    variant::run("emoji", Parts { emoji: true, ..variant::FULL })
}
