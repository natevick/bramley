use ui_baseline::variant::{self, Parts};

fn main() -> eframe::Result {
    variant::run("noicons", Parts { icons: false, ..variant::FULL })
}
